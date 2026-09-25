//! The engine's one door: a named pipe the browser talks to.
//!
//! One JSON message per line, both ways.
//!
//! - Call: `{"id": 7, "method": "compute_hashes", "params": {…}}`
//! - Answer: `{"id": 7, "result": …}` or `{"id": 7, "error": {"message": "…", "data": …}}`
//! - Event, whenever the engine has one: `{"event": "fm-progress", "payload": …}`
//!
//! Method names are the command names Workspace's pages already `invoke`, so
//! the pages work unchanged; `engine.hello`, `engine.methods` and
//! `engine.exit` are the pipe's own. Calls run concurrently and answer in
//! whatever order they finish.
//!
//! The pipe's access list names only the user running the engine: no other
//! account on the machine — and no remote client — can open it. The first
//! instance claims the name, so a second engine for the same world simply
//! fails to start, and the browser talks to the first.

use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::ipc::{Handler, Invoke};
use tauri::AppHandle;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::{broadcast, mpsc};

pub struct Options {
    pub pipe: String,
    /// How long the engine stays up once the last client has gone.
    pub linger: Duration,
}

pub fn serve(app: AppHandle, handler: Handler, options: Options) -> std::io::Result<()> {
    tauri::async_runtime::runtime().block_on(listen(app, handler, options))
}

async fn listen(app: AppHandle, handler: Handler, options: Options) -> std::io::Result<()> {
    let path = format!(r"\\.\pipe\{}", options.pipe);
    let (events, _) = broadcast::channel::<Arc<str>>(1024);
    {
        let events = events.clone();
        app.set_event_sink(move |event, payload| {
            let line = json!({ "event": event, "payload": payload }).to_string();
            let _ = events.send(Arc::from(line));
        });
    }
    let (quit_tx, mut quit_rx) = mpsc::unbounded_channel::<i32>();
    app.set_exit_handler(move |code| {
        let _ = quit_tx.send(code);
    });

    let security = UserOnly::new()?;
    let clients = Arc::new(AtomicUsize::new(0));
    let (gone_tx, mut gone_rx) = mpsc::unbounded_channel::<()>();
    // Claiming the name fails with "access denied" when another engine
    // already holds it; say what that means.
    let mut server = create(&path, &security, true).map_err(|error| {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("another engine is already answering on {path}"),
            )
        } else {
            error
        }
    })?;
    eprintln!("kil-engine: listening on {path} ({} commands)", handler.names.len());

    // When the last client left. The wait for `linger` is a branch of its
    // own, so the pipe keeps answering while it runs: a client that comes
    // back in time finds the engine, rather than one that stopped listening.
    let mut idle_since: Option<tokio::time::Instant> = None;
    loop {
        let deadline = idle_since.map(|since| since + options.linger);
        tokio::select! {
            connected = server.connect() => {
                connected?;
                let client = std::mem::replace(&mut server, create(&path, &security, false)?);
                clients.fetch_add(1, Ordering::SeqCst);
                idle_since = None;
                let (app, events, clients, gone) = (app.clone(), events.subscribe(), clients.clone(), gone_tx.clone());
                tokio::spawn(async move {
                    serve_client(client, app, handler.find, handler.names, events).await;
                    clients.fetch_sub(1, Ordering::SeqCst);
                    let _ = gone.send(());
                });
            }
            Some(()) = gone_rx.recv() => {
                if clients.load(Ordering::SeqCst) == 0 {
                    idle_since = Some(tokio::time::Instant::now());
                }
            }
            _ = tokio::time::sleep_until(deadline.unwrap_or_else(tokio::time::Instant::now)), if deadline.is_some() => {
                if clients.load(Ordering::SeqCst) == 0 {
                    eprintln!("kil-engine: no clients left, stopping");
                    return Ok(());
                }
                idle_since = None;
            }
            Some(code) = quit_rx.recv() => {
                eprintln!("kil-engine: asked to stop ({code})");
                return Ok(());
            }
        }
    }
}

fn create(path: &str, security: &UserOnly, first: bool) -> std::io::Result<NamedPipeServer> {
    let mut options = ServerOptions::new();
    options.first_pipe_instance(first).reject_remote_clients(true);
    unsafe { options.create_with_security_attributes_raw(path, security.attributes()) }
}

async fn serve_client(
    pipe: NamedPipeServer,
    app: AppHandle,
    find: fn(&str) -> Option<tauri::ipc::CommandFn>,
    names: &'static [&'static str],
    mut events: broadcast::Receiver<Arc<str>>,
) {
    let (read, mut write) = tokio::io::split(pipe);
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Arc<str>>();

    // One writer: answers and events interleave, whole lines at a time.
    let writer = tokio::spawn(async move {
        loop {
            let line = tokio::select! {
                line = out_rx.recv() => match line { Some(line) => line, None => break },
                event = events.recv() => match event {
                    Ok(line) => line,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
            };
            if write.write_all(line.as_bytes()).await.is_err() || write.write_all(b"\n").await.is_err() {
                break;
            }
        }
    });

    let mut lines = BufReader::new(read).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let out = out_tx.clone();
        let app = app.clone();
        tokio::spawn(async move {
            let reply = answer(&line, app, find, names).await;
            let _ = out.send(Arc::from(reply.to_string()));
        });
    }
    drop(out_tx);
    writer.abort();
}

async fn answer(
    line: &str,
    app: AppHandle,
    find: fn(&str) -> Option<tauri::ipc::CommandFn>,
    names: &'static [&'static str],
) -> Value {
    let request: Value = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(error) => return json!({ "id": null, "error": { "message": format!("not JSON: {error}") } }),
    };
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str).unwrap_or_default();
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

    let outcome = match method {
        "engine.hello" => Ok(json!({
            "engine": "kil-engine",
            "version": env!("CARGO_PKG_VERSION"),
            "pid": std::process::id(),
            "commands": names.len(),
        })),
        "engine.methods" => Ok(json!(names)),
        "engine.exit" => {
            app.exit(0);
            Ok(Value::Null)
        }
        _ => match find(method) {
            Some(command) => command(Invoke { app, args: params }).await,
            None => Err(Value::String(format!("no command called `{method}`"))),
        },
    };

    match outcome {
        Ok(result) => json!({ "id": id, "result": result }),
        Err(data) => {
            let message = match &data {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            json!({ "id": id, "error": { "message": message, "data": data } })
        }
    }
}

/// A security descriptor granting the current user, and no one else, full
/// access — the pipe's whole access list.
struct UserOnly {
    descriptor: windows::Win32::Security::PSECURITY_DESCRIPTOR,
    attributes: windows::Win32::Security::SECURITY_ATTRIBUTES,
}

// The descriptor is only read, by CreateNamedPipe, after it's made.
unsafe impl Send for UserOnly {}
unsafe impl Sync for UserOnly {}

impl UserOnly {
    fn new() -> std::io::Result<Self> {
        use windows::core::{PCWSTR, PWSTR};
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::Security::Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SDDL_REVISION_1,
        };
        use windows::Win32::Security::{
            GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
            TOKEN_USER,
        };
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

        let io = |e: windows::core::Error| std::io::Error::other(e.to_string());
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(io)?;
            let mut needed = 0u32;
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut needed);
            let mut buffer = vec![0u8; needed as usize];
            let read = GetTokenInformation(
                token,
                TokenUser,
                Some(buffer.as_mut_ptr().cast()),
                needed,
                &mut needed,
            );
            let _ = CloseHandle(token);
            read.map_err(io)?;
            let user = &*(buffer.as_ptr() as *const TOKEN_USER);

            let mut sid_text = PWSTR::null();
            ConvertSidToStringSidW(user.User.Sid, &mut sid_text).map_err(io)?;
            let sid = sid_text.to_string().map_err(|e| std::io::Error::other(e.to_string()))?;
            let _ = windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(
                sid_text.0.cast(),
            ));

            // Protected DACL, one entry: generic-all for this user.
            let sddl: Vec<u16> = format!("D:P(A;;GA;;;{sid})").encode_utf16().chain([0]).collect();
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
            .map_err(io)?;
            Ok(UserOnly {
                descriptor,
                attributes: SECURITY_ATTRIBUTES {
                    nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                    lpSecurityDescriptor: descriptor.0,
                    bInheritHandle: false.into(),
                },
            })
        }
    }

    fn attributes(&self) -> *mut std::ffi::c_void {
        &self.attributes as *const _ as *mut std::ffi::c_void
    }
}

impl Drop for UserOnly {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(self.descriptor.0));
        }
    }
}
