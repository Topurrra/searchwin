# Provenance

Every tree copied into this repo from another project. The source repos are never modified.

| Path here | Source | Revision | Notes |
|---|---|---|---|
| `Engine/` | KeepItLocal-Workspace `src-tauri/` (src, tests, Cargo.toml/lock, build.rs) | `df919f2` (2026-09-18) | `src/core/resources.rs` **left out on purpose**: it came from KeepItLocal Redact's predecessor, which stays out of Search. `reference/` holds `tauri.conf.json`, capabilities and the runtime READMEs, for reference only |
| `Tools/` | KeepItLocal-Workspace `src/`, `static/`, package and build config | `df919f2` | |
| `docs/workspace/` | KeepItLocal-Workspace README, KeepItLocal.md, roadmap, docs | `df919f2` | `CLAUDE.md`/`AGENTS.md` renamed `workspace-*.md` so agents don't load them as instructions |
| `Extensions/fishcatcher/` | FishCatcher `src/`, LICENSE, README, CHANGELOG | `04f31ab` (2026-08-23) | MIT. `src/vendor/jsQR` is Apache-2.0 |

## Third-party binaries (not in the repo yet)
When the packs ship, add a NOTICE for each:
- Vosk (`libvosk.dll`), Apache-2.0
- Tesseract + Leptonica, Apache-2.0 / BSD
- pdfium, BSD-3 / Apache-2.0
- all-MiniLM-L6-v2, Apache-2.0
- u2netp, Apache-2.0 (confirm)

FFmpeg is never bundled.
