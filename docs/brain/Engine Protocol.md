---
tags: [searchwin, engine, protocol]
updated: 2026-09-25
---

# Engine Protocol

Back to [[README]] · [[Master Plan]] · Code: `Engine/src/server.rs`

`kil-engine.exe` is KeepItLocal Workspace's Rust core, running headless. The
browser starts it and talks to it over **one named pipe**.

## Starting it

```
kil-engine.exe --data-dir <folder> --pipe <name> [--linger <seconds>]
```

| Argument | Default | Browser passes |
|---|---|---|
| `--data-dir` | `%LOCALAPPDATA%\Search\Engine` | `…\Search\Engine`, or `…\Search (<world>)\Engine` in a test world |
| `--pipe` | `search-engine` | `search-engine`, or `search-engine-<world>` |
| `--linger` | `15` | How long it stays up after the last client disconnects |

Exit codes:
- **0** — stopped normally (asked to, or no clients left for `--linger` seconds);
- **1** — failed;
- **2** — another engine already answers on that pipe, so talk to that one.

Workspace's worker modes (`--index-worker …`, `--keepitlocal-archive-worker …`)
still work: the engine relaunches itself for those.

## Security
- The pipe's access list has **one entry**: the user running the engine,
  full control, with inheritance blocked.
- Remote clients are rejected.
- The first instance claims the name, so nothing else can impersonate the engine
  for that world while it runs.

## Messages
One JSON object per line (UTF-8, `\n`), in both directions.

```jsonc
// call
{"id": 7, "method": "encode_decode", "params": {"algorithm": "base64", "mode": "encode", "input": "hi"}}
// answer
{"id": 7, "result": "aGk="}
// failure: `data` is the command's own error value, `message` its text
{"id": 7, "error": {"message": "Unknown algorithm: nope", "data": "Unknown algorithm: nope"}}
// event, whenever the engine has one (progress, clipboard changes…)
{"event": "fm-progress", "payload": {…}}
```

- **Method names are Workspace's command names**, so the tool pages' `invoke("…")` calls map one to one.
- **Parameters** are looked up by camelCase name, as Tauri did (`operationId`); snake_case is accepted too.
- **Calls run concurrently** and answer in whatever order they finish. Match answers by `id`.
- **Events go to every connected client.**

### The pipe's own methods
| Method | Result |
|---|---|
| `engine.hello` | `{engine, version, pid, commands}` |
| `engine.methods` | the names of every command |
| `engine.exit` | stops the engine |

### `shell:request` events
Commands that drove Workspace's own windows, tray and hotkeys now ask the
browser instead. The payload is `{"request": "<what>", "detail": …}`:
- `show-field` / `hide-field`
- `show-clipboard`
- `show-voice`
- `show-quick-note`
- `show-toast`
- `hotkey:<name>` (the settings to register)
- `suspend-hotkeys` / `resume-hotkeys`
- `listening`

### Background services
Workspace started these at launch. In Search the browser starts them when a
feature is switched on:
- `start_clipboard_listener`
- `start_search_services` (the index scheduler and warm readers)
- the time tracker
- …

## How it's built
- `Engine/compat/tauri` + `tauri-macros` is a stand-in for the few Tauri APIs
  the commands use:
  - `#[tauri::command]` generates a hidden module holding a JSON entry point;
  - `generate_handler!` builds the name → entry table;
  - `AppHandle` provides state, events, paths and exit;
  - windows don't exist.
- Workspace's command code is **unchanged**, except that three files import
  `core::throttle` (Search's own) instead of the file that was left out.
- Heavy features stay behind cargo features: `vosk`, `ocr-leptess`,
  `semantic`, `screenrec`, `image-background-removal`. For now, build with
  `--no-default-features`.
- `ravif` is built without NASM assembly (slower AVIF encoding). Turn it back on for releases.
