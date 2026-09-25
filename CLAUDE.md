# Search for Windows

A small, fast, private browser (C# / .NET 9 / WinUI 3 / WebView2, Native AOT). It's
becoming "a browser first, then a Raycast-like launcher, but better".

**Read first:** `docs/brain/README.md`, then `docs/brain/Master Plan.md`. If you're a
cloud agent, also read `docs/brain/Cloud Agent Brief.md`: it has your tasks and the
limits of a Linux environment.

## Layout
- `Search/`: the WinUI browser (the shell)
- `Search.Kit/`: portable, UI-free C# logic, testable on Linux (created in task T1)
- `Engine/`: a copy of KeepItLocal Workspace's Rust engine, becoming the headless `kil-engine`
- `Tools/`: a copy of Workspace's Svelte tool screens, becoming pages in tabs
- `Extensions/fishcatcher/`: a copy of FishCatcher (until the native port)
- `docs/brain/`: a mirror of the project's Obsidian notes; `docs/workspace/`: Workspace's own docs, for reference
- `third_party/PROVENANCE.md`: where every copied tree came from

## Rules
- Work in this repo only. Never modify other repositories.
- **Nothing from KeepItLocal Redact**, in any form. `Engine/src/core/resources.rs` was left out on purpose.
- Commit messages as short as possible, one line. **Never** add `Co-Authored-By` or AI attribution.
- Native AOT safe: no reflection, source-generated JSON (`JsonNode` for open-ended data), WinRT casts via `.As<T>()`.
- Nothing may load before the first window. Field logic stays under 5 ms per keystroke.
- No network calls except: downloads the user starts, and the background updates of public protection data — Shields filter lists (easylist.to) and FishCatcher's signed feed — which are ON by default, go only to their documented publishers, and never send anything about the user's browsing.
- Test the release (AOT) build for UI changes, in a test world (`SEARCH_PROBE=<name>`), never the real profile.
- Tests obey `SKILL.md` (test-audit): every new or changed test passes its authoring gate (the four
  questions, no junk patterns), and a bug's regression test must fail on the pre-fix code. Its
  OpenClaw commands map here to: `dotnet test Search.Kit.Tests`, `node --test Search/Assets/js/tests/`,
  `pnpm vitest run` in `Tools/`, `cargo test` in `Engine/`, then `git diff --check`; `$autoreview`
  means a review pass over the diff.
- After a work session, update `docs/brain/` (Log, Lessons Learned, Decisions).
