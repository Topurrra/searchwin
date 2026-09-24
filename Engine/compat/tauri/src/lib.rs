//! Search's stand-in for the few parts of Tauri the engine's commands use.
//!
//! The engine came from KeepItLocal Workspace, a Tauri app. In Search it runs
//! headless, beside the browser, and answers over a pipe; the browser owns
//! every window. Rather than rewrite hundreds of commands, the engine keeps
//! calling what it always called — `AppHandle`, `State`, `emit`, `path()` —
//! and this crate answers:
//!
//! - state is a type-keyed map, managed once at start;
//! - events go to whoever is listening on the pipe;
//! - paths point at Search's own data folder, never Workspace's;
//! - windows don't exist here: `get_webview_window` finds none, and a window
//!   that is asked to be built says it is the browser's job.

pub use tauri_macros::{command, generate_handler};

use serde::Serialize;
use serde_json::Value;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

// ─── Errors ─────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum Error {
    /// Something only the browser's shell can do (windows, tray, dialogs).
    NotHere(&'static str),
    Join(String),
    Io(std::io::Error),
    Json(serde_json::Error),
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotHere(what) => write!(f, "{what} is the browser's job, not the engine's"),
            Error::Join(message) => write!(f, "background task failed: {message}"),
            Error::Io(error) => write!(f, "{error}"),
            Error::Json(error) => write!(f, "{error}"),
            Error::Other(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Error::Io(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Error::Json(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

// ─── The app handle ─────────────────────────────────────────────────────

type Sink = Arc<dyn Fn(&str, Value) + Send + Sync>;

struct Inner {
    states: RwLock<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
    paths: PathResolver,
    events: RwLock<Option<Sink>>,
    exit: RwLock<Option<Arc<dyn Fn(i32) + Send + Sync>>>,
}

/// The engine, as the commands see it. Cheap to clone.
#[derive(Clone)]
pub struct AppHandle {
    inner: Arc<Inner>,
}

impl AppHandle {
    /// Made once, by the engine's `run`, with the folder its data lives in
    /// and the folder its bundled resources (runtimes, models) sit in.
    pub fn new(data_dir: PathBuf, resource_dir: PathBuf) -> Self {
        AppHandle {
            inner: Arc::new(Inner {
                states: RwLock::new(HashMap::new()),
                paths: PathResolver { data_dir, resource_dir },
                events: RwLock::new(None),
                exit: RwLock::new(None),
            }),
        }
    }

    /// Where `emit` sends its events: the pipe server, once it is up.
    pub fn set_event_sink(&self, sink: impl Fn(&str, Value) + Send + Sync + 'static) {
        *self.inner.events.write().unwrap_or_else(|p| p.into_inner()) = Some(Arc::new(sink));
    }

    /// What `exit` does: the pipe server stops, the process ends.
    pub fn set_exit_handler(&self, handler: impl Fn(i32) + Send + Sync + 'static) {
        *self.inner.exit.write().unwrap_or_else(|p| p.into_inner()) = Some(Arc::new(handler));
    }

    pub fn exit(&self, code: i32) {
        let handler = self.inner.exit.read().unwrap_or_else(|p| p.into_inner()).clone();
        match handler {
            Some(handler) => handler(code),
            None => std::process::exit(code),
        }
    }

    /// The browser starts the engine again when it next needs it.
    pub fn restart(&self) {
        self.exit(0)
    }

    fn send(&self, event: &str, payload: Value) {
        let sink = self.inner.events.read().unwrap_or_else(|p| p.into_inner()).clone();
        if let Some(sink) = sink {
            sink(event, payload);
        }
    }
}

// ─── Manager: state, paths, windows ─────────────────────────────────────

pub trait Manager {
    fn app_handle(&self) -> &AppHandle;

    fn manage<T: Send + Sync + 'static>(&self, state: T) -> bool {
        let mut states = self.app_handle().inner.states.write().unwrap_or_else(|p| p.into_inner());
        let id = TypeId::of::<T>();
        if states.contains_key(&id) {
            return false;
        }
        states.insert(id, Arc::new(state));
        true
    }

    fn try_state<T: Send + Sync + 'static>(&self) -> Option<State<'_, T>> {
        let states = self.app_handle().inner.states.read().unwrap_or_else(|p| p.into_inner());
        let found = states.get(&TypeId::of::<T>())?.clone();
        let typed = found.downcast::<T>().ok()?;
        Some(State { inner: typed, _borrow: PhantomData })
    }

    /// Like Tauri, a state that was never managed is a programming error.
    fn state<T: Send + Sync + 'static>(&self) -> State<'_, T> {
        self.try_state::<T>().unwrap_or_else(|| {
            panic!("state {} was never managed by the engine", std::any::type_name::<T>())
        })
    }

    fn path(&self) -> &PathResolver {
        &self.app_handle().inner.paths
    }

    /// The engine has no windows: the browser draws everything.
    fn get_webview_window(&self, _label: &str) -> Option<WebviewWindow> {
        None
    }

    fn get_window(&self, _label: &str) -> Option<Window> {
        None
    }

    fn webview_windows(&self) -> HashMap<String, WebviewWindow> {
        HashMap::new()
    }
}

impl Manager for AppHandle {
    fn app_handle(&self) -> &AppHandle {
        self
    }
}

// ─── Emitter: events out to the browser ─────────────────────────────────

pub trait Emitter {
    fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<()>;

    fn emit_to<S: Serialize + Clone>(&self, _target: &str, event: &str, payload: S) -> Result<()> {
        self.emit(event, payload)
    }
}

impl Emitter for AppHandle {
    fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<()> {
        self.send(event, serde_json::to_value(payload)?);
        Ok(())
    }
}

// ─── State ──────────────────────────────────────────────────────────────

/// A managed value. Holds its own reference, so it can cross an `.await`.
pub struct State<'a, T: Send + Sync + 'static> {
    inner: Arc<T>,
    _borrow: PhantomData<&'a ()>,
}

impl<T: Send + Sync + 'static> State<'_, T> {
    pub fn inner(&self) -> &T {
        &self.inner
    }
}

impl<T: Send + Sync + 'static> Deref for State<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.inner
    }
}

impl<T: Send + Sync + 'static> Clone for State<'_, T> {
    fn clone(&self) -> Self {
        State { inner: self.inner.clone(), _borrow: PhantomData }
    }
}

// ─── Paths ──────────────────────────────────────────────────────────────

/// Every "app" folder is Search's engine folder: one place, per test world,
/// and never the folders a standalone Workspace install uses.
pub struct PathResolver {
    data_dir: PathBuf,
    resource_dir: PathBuf,
}

impl PathResolver {
    fn ensure(&self, dir: PathBuf) -> Result<PathBuf> {
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn app_data_dir(&self) -> Result<PathBuf> {
        self.ensure(self.data_dir.clone())
    }

    pub fn app_local_data_dir(&self) -> Result<PathBuf> {
        self.ensure(self.data_dir.clone())
    }

    pub fn app_config_dir(&self) -> Result<PathBuf> {
        self.ensure(self.data_dir.clone())
    }

    pub fn app_cache_dir(&self) -> Result<PathBuf> {
        self.ensure(self.data_dir.join("cache"))
    }

    pub fn app_log_dir(&self) -> Result<PathBuf> {
        self.ensure(self.data_dir.join("logs"))
    }

    pub fn resource_dir(&self) -> Result<PathBuf> {
        Ok(self.resource_dir.clone())
    }

    pub fn temp_dir(&self) -> Result<PathBuf> {
        Ok(std::env::temp_dir())
    }

    pub fn home_dir(&self) -> Result<PathBuf> {
        dirs::home_dir().ok_or(Error::Other("no home folder".into()))
    }

    /// The person's own folders, wherever Windows has moved them (OneDrive…).
    pub fn document_dir(&self) -> Result<PathBuf> {
        dirs::document_dir().ok_or(Error::Other("no Documents folder".into()))
    }

    pub fn download_dir(&self) -> Result<PathBuf> {
        dirs::download_dir().ok_or(Error::Other("no Downloads folder".into()))
    }

    pub fn desktop_dir(&self) -> Result<PathBuf> {
        dirs::desktop_dir().ok_or(Error::Other("no Desktop folder".into()))
    }

    pub fn picture_dir(&self) -> Result<PathBuf> {
        dirs::picture_dir().ok_or(Error::Other("no Pictures folder".into()))
    }

    pub fn video_dir(&self) -> Result<PathBuf> {
        dirs::video_dir().ok_or(Error::Other("no Videos folder".into()))
    }

    pub fn audio_dir(&self) -> Result<PathBuf> {
        dirs::audio_dir().ok_or(Error::Other("no Music folder".into()))
    }
}

// ─── Windows: not here ──────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, serde::Deserialize)]
pub struct PhysicalPosition<T> {
    pub x: T,
    pub y: T,
}

impl<T> PhysicalPosition<T> {
    pub fn new(x: T, y: T) -> Self {
        PhysicalPosition { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, serde::Deserialize)]
pub struct PhysicalSize<T> {
    pub width: T,
    pub height: T,
}

impl<T> PhysicalSize<T> {
    pub fn new(width: T, height: T) -> Self {
        PhysicalSize { width, height }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LogicalPosition<T> {
    pub x: T,
    pub y: T,
}

impl<T> LogicalPosition<T> {
    pub fn new(x: T, y: T) -> Self {
        LogicalPosition { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LogicalSize<T> {
    pub width: T,
    pub height: T,
}

impl<T> LogicalSize<T> {
    pub fn new(width: T, height: T) -> Self {
        LogicalSize { width, height }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Size {
    Physical(PhysicalSize<u32>),
    Logical(LogicalSize<f64>),
}

#[derive(Clone, Copy, Debug)]
pub enum Position {
    Physical(PhysicalPosition<i32>),
    Logical(LogicalPosition<f64>),
}

impl From<PhysicalPosition<i32>> for Position {
    fn from(p: PhysicalPosition<i32>) -> Self {
        Position::Physical(p)
    }
}

impl From<LogicalPosition<f64>> for Position {
    fn from(p: LogicalPosition<f64>) -> Self {
        Position::Logical(p)
    }
}

impl From<PhysicalSize<u32>> for Size {
    fn from(s: PhysicalSize<u32>) -> Self {
        Size::Physical(s)
    }
}

impl From<LogicalSize<f64>> for Size {
    fn from(s: LogicalSize<f64>) -> Self {
        Size::Logical(s)
    }
}

/// A display, as far as the code that positions windows needs one.
#[derive(Clone, Debug, Default)]
pub struct Monitor {
    size: PhysicalSize<u32>,
    position: PhysicalPosition<i32>,
    scale_factor: f64,
}

impl Monitor {
    pub fn size(&self) -> &PhysicalSize<u32> {
        &self.size
    }
    pub fn position(&self) -> &PhysicalPosition<i32> {
        &self.position
    }
    pub fn scale_factor(&self) -> f64 {
        self.scale_factor
    }
}

/// A window the engine can't have. It exists so code that would have moved
/// one still compiles; nothing ever hands one out.
#[derive(Clone)]
pub struct WebviewWindow {
    label: String,
    app: Option<AppHandle>,
}

pub type Window = WebviewWindow;

impl WebviewWindow {
    pub fn label(&self) -> &str {
        &self.label
    }
    pub fn show(&self) -> Result<()> {
        Err(Error::NotHere("showing a window"))
    }
    pub fn hide(&self) -> Result<()> {
        Ok(())
    }
    pub fn close(&self) -> Result<()> {
        Ok(())
    }
    pub fn destroy(&self) -> Result<()> {
        Ok(())
    }
    pub fn set_focus(&self) -> Result<()> {
        Ok(())
    }
    pub fn is_visible(&self) -> Result<bool> {
        Ok(false)
    }
    pub fn is_focused(&self) -> Result<bool> {
        Ok(false)
    }
    pub fn is_minimized(&self) -> Result<bool> {
        Ok(false)
    }
    pub fn unminimize(&self) -> Result<()> {
        Ok(())
    }
    pub fn set_position<P: Into<Position>>(&self, _position: P) -> Result<()> {
        Ok(())
    }
    pub fn set_size<S: Into<Size>>(&self, _size: S) -> Result<()> {
        Ok(())
    }
    pub fn set_always_on_top(&self, _on: bool) -> Result<()> {
        Ok(())
    }
    pub fn set_ignore_cursor_events(&self, _ignore: bool) -> Result<()> {
        Ok(())
    }
    pub fn set_content_protected(&self, _protected: bool) -> Result<()> {
        Ok(())
    }
    pub fn primary_monitor(&self) -> Result<Option<Monitor>> {
        Ok(None)
    }
    pub fn current_monitor(&self) -> Result<Option<Monitor>> {
        Ok(None)
    }
    pub fn scale_factor(&self) -> Result<f64> {
        Ok(1.0)
    }
    pub fn outer_position(&self) -> Result<PhysicalPosition<i32>> {
        Ok(PhysicalPosition::default())
    }
    pub fn outer_size(&self) -> Result<PhysicalSize<u32>> {
        Ok(PhysicalSize::default())
    }
    pub fn inner_size(&self) -> Result<PhysicalSize<u32>> {
        Ok(PhysicalSize::default())
    }
    pub fn hwnd(&self) -> Result<Hwnd> {
        Err(Error::NotHere("a window handle"))
    }
    pub fn eval(&self, _script: &str) -> Result<()> {
        Err(Error::NotHere("running script in a window"))
    }
}

/// Stands in for Tauri's `HWND` wrapper.
#[derive(Clone, Copy, Debug)]
pub struct Hwnd(pub isize);

impl Manager for WebviewWindow {
    fn app_handle(&self) -> &AppHandle {
        self.app.as_ref().expect("a window with no app")
    }
}

impl Emitter for WebviewWindow {
    fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<()> {
        match &self.app {
            Some(app) => app.emit(event, payload),
            None => Ok(()),
        }
    }
}

pub enum WebviewUrl {
    App(PathBuf),
    External(String),
}

/// Builds nothing: asking the engine for a window is asking the wrong
/// process. The command that asked gets an error it already handles.
pub struct WebviewWindowBuilder;

impl WebviewWindowBuilder {
    pub fn new<M: Manager, L: Into<String>>(_manager: &M, _label: L, _url: WebviewUrl) -> Self {
        WebviewWindowBuilder
    }
    pub fn build(self) -> Result<WebviewWindow> {
        Err(Error::NotHere("opening a window"))
    }
}

macro_rules! builder_passthrough {
    ($($name:ident($($arg:ident: $ty:ty),*)),* $(,)?) => {
        impl WebviewWindowBuilder {
            $(pub fn $name(self, $(_: $ty),*) -> Self { let _ = ($(stringify!($arg)),*); self })*
        }
    };
}

builder_passthrough!(
    title(title: &str),
    inner_size(width: f64, height: f64),
    min_inner_size(width: f64, height: f64),
    position(x: f64, y: f64),
    decorations(on: bool),
    transparent(on: bool),
    shadow(on: bool),
    resizable(on: bool),
    maximized(on: bool),
    visible(on: bool),
    focused(on: bool),
    always_on_top(on: bool),
    skip_taskbar(on: bool),
    content_protected(on: bool),
    center(),
);

// ─── The runtime ────────────────────────────────────────────────────────

pub mod async_runtime {
    use super::Error;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::OnceLock;
    use std::task::{Context, Poll};

    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

    /// One multi-threaded runtime for the whole engine, made on first use.
    pub fn runtime() -> &'static tokio::runtime::Runtime {
        RUNTIME.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("engine")
                .build()
                .expect("the engine's runtime")
        })
    }

    pub struct JoinHandle<T>(tokio::task::JoinHandle<T>);

    impl<T> JoinHandle<T> {
        pub fn abort(&self) {
            self.0.abort()
        }
    }

    impl<T> Future for JoinHandle<T> {
        type Output = Result<T, Error>;
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            Pin::new(&mut self.0).poll(cx).map(|out| out.map_err(|e| Error::Join(e.to_string())))
        }
    }

    pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        JoinHandle(runtime().spawn(future))
    }

    pub fn spawn_blocking<F, R>(work: F) -> JoinHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        JoinHandle(runtime().spawn_blocking(work))
    }

    pub fn block_on<F: Future>(future: F) -> F::Output {
        match tokio::runtime::Handle::try_current() {
            Ok(_) => tokio::task::block_in_place(|| runtime().block_on(future)),
            Err(_) => runtime().block_on(future),
        }
    }
}

// ─── What the macros generate against ───────────────────────────────────

pub mod ipc {
    use super::{AppHandle, WebviewWindow};
    use serde::de::DeserializeOwned;
    use serde::Serialize;
    use serde_json::Value;
    use std::future::Future;
    use std::pin::Pin;

    /// One call: the engine, and the arguments as the caller sent them.
    pub struct Invoke {
        pub app: AppHandle,
        pub args: Value,
    }

    pub type CommandFuture = Pin<Box<dyn Future<Output = Result<Value, Value>> + Send>>;
    pub type CommandFn = fn(Invoke) -> CommandFuture;

    /// Every command, by name.
    pub struct Handler {
        pub names: &'static [&'static str],
        pub find: fn(&str) -> Option<CommandFn>,
    }

    /// An argument by its camelCase name, or its snake_case one. A missing
    /// argument is `null`, which is what an `Option` wants.
    pub fn arg<T: DeserializeOwned>(args: &Value, camel: &str, snake: &str) -> Result<T, Value> {
        let value = args.get(camel).or_else(|| args.get(snake)).cloned().unwrap_or(Value::Null);
        serde_json::from_value(value)
            .map_err(|error| Value::String(format!("argument `{camel}`: {error}")))
    }

    pub fn from_value<T: Serialize>(value: T) -> Result<Value, Value> {
        serde_json::to_value(value).map_err(|error| Value::String(error.to_string()))
    }

    pub fn from_result<T: Serialize, E: Serialize>(result: Result<T, E>) -> Result<Value, Value> {
        match result {
            Ok(value) => from_value(value),
            Err(error) => Err(serde_json::to_value(error)
                .unwrap_or_else(|e| Value::String(e.to_string()))),
        }
    }

    /// A managed value for a command parameter. It owns its reference, so it
    /// can go with the command onto the blocking pool.
    pub fn state<T: Send + Sync + 'static>(app: &AppHandle) -> super::State<'static, T> {
        let found = super::Manager::state::<T>(app);
        super::State { inner: found.inner.clone(), _borrow: std::marker::PhantomData }
    }

    pub fn panicked(error: super::Error) -> Value {
        Value::String(format!("the command stopped unexpectedly: {error}"))
    }

    /// Parameters the engine fills in itself.
    pub trait FromInvoke {
        fn from_invoke(invoke: &Invoke) -> Self;
    }

    impl FromInvoke for WebviewWindow {
        fn from_invoke(invoke: &Invoke) -> Self {
            WebviewWindow { label: "browser".into(), app: Some(invoke.app.clone()) }
        }
    }
}
