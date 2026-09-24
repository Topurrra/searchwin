# kil-engine

KeepItLocal Workspace's Rust core, running headless beside Search. The browser starts it,
talks to it over a named pipe, and owns every window and hotkey.

```powershell
cargo build --no-default-features            # the light build (no Vosk/Tesseract/ONNX/recorder)
cargo test --no-default-features --lib
target\debug\kil-engine.exe --data-dir <dir> --pipe <name> [--linger <secs>]
```

- `src/server.rs`: the pipe (JSON-RPC, one message per line, locked to the current user)
- `src/shell.rs`: what used to drive Workspace's windows/tray/hotkeys, now requests to the browser
- `src/lib.rs`: startup and the command table
- `src/commands/`: Workspace's commands, unchanged
- `compat/`: the small stand-in for the Tauri API those commands use
- `reference/`: Workspace's original `lib.rs`, `tauri.conf.json` and runtime notes, for reference only

The protocol is in `docs/brain/Engine Protocol.md`.
