---
tags: [searchwin, changelog]
updated: 2026-09-24
---

# Changelog

Back to [[README]] · Newest first. Short hashes are from `searchwin`'s git.

## 1.0.0: 2026-09-24

### Fixes after first use
- `b7916e4` **Release builds carry WinUI's resource index.** Hovering View in
  the hamburger menu no longer crashes the app, and dialogs are rounded and
  Fluent-styled again.
- `686f80f` Fixes from going through everything built:
  - extension popups see the current tab
  - the find counter
  - spelling setting
  - the curtain picker re-arming
  - the New Space dialog waits for a name
  - Welcome import text
- `fc4e534` **Hamburger menu** for what the Mac keeps in its menu bar; tabs shortened to 160 px.
- `2cb44b8` One set of window buttons (it was doubled in AOT); the field's glow breathes without flickering.

### Shipping
- `e681dff` NSIS installer (per-user, bundles the WebView2 bootstrapper), plus `PLAN.md`.
- `7113bff`, `7667374` **Native AOT**: a 15 MB exe and no .NET runtime (73 MB installed).
- `c70536c` README: where Windows differs, and how to drive the app.

### Features
- `1cda07d` Bench over a named pipe; the store's live "Add to Chrome" taken over; the passwords list redraws.
- `1e7387e`, `d6e818c`, `9c76a67` **Spaces** and the **bench**.
- `eca7a6c`, `ee9cd22` **Chrome extensions** on WebView2's engine.
- `26475bd`, `e4711d5` **Passwords**: the vault (Credential Manager), the sign-in relay, the list, import.
- `80b558c`, `9588c71`, `70a93ab` **Panels**: history, downloads, bookmarks, settings, welcome.
- `781d2f7`, `8f3722e`, `6c07901`, `0e84405` **Blocker, curtain (hide), reading mode, floating video**; zoom against the display scale.
- `4fa5561` Keys from inside pages (keyboard hook), a drawn bookmark icon, the field's completion.
- `91345ce` → `2f36242` The foundation: the window, tabs, field and page; stubs per feature; build script and README.
