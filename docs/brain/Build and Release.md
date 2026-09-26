---
tags: [searchwin, build]
updated: 2026-09-27
---

# Build and Release

Back to [[README]] · Problems we hit: [[Lessons Learned#Build and Native AOT]]

## Prerequisites

- .NET 9 SDK (`winget install Microsoft.DotNet.SDK.9`, or `dotnet-install.ps1 -Channel 9.0` into `%USERPROFILE%\.dotnet`; the scripts find either)
- For **Native AOT**: Visual Studio 2022 or its Build Tools with *Desktop development with C++* (the MSVC linker)
- For the installer: **NSIS 3** (`makensis.exe` in Program Files)
- For the engine: **Rust** (stable, MSVC), and **NASM** for fast AVIF encoding (optional; the build falls back without it). On this PC: NASM 3.02 in `%LOCALAPPDATA%\Programs\nasm`, on the user PATH.
- For the tool pages: **Node 22+** and **pnpm 9.15.9** (pinned in Tools/package.json)
- WebView2 runtime (already on Windows 11)

## Commands (run in `searchwin\`)

```powershell
dotnet build Search\Search.csproj   # debug → Search\bin\...\Debug\...\Search.exe (runs in test world "test")
.\build.ps1                          # release → build\Search\Search.exe (Native AOT if MSVC is present, else ReadyToRun)
.\build.ps1 -Zip                     # + build\Search-<ver>-x64.zip
.\build.ps1 -Installer               # + build\Search-Setup-<ver>-x64.exe
# Arm64 packaging is deferred: the scripts reject mixed-architecture builds.
```

What `build.ps1` does:

1. Validates the SDK, required component toolchains, and workspace output before replacing `build\Search`.
2. Checks for MSVC with `vswhere`. If found (x64), runs `publish-aot.cmd`,
   which calls `vcvarsall.bat x64` and runs `dotnet publish -p:PublishAot=true
   -p:IlcUseEnvironmentalTools=true -p:PublishReadyToRun=false -p:DebugType=none`.
3. Otherwise runs `dotnet publish -p:PublishReadyToRun=true` (about 215 MB, because it carries the .NET runtime).
4. Moves `*.pdb` out to `build\`: symbols are kept but not shipped.
5. Builds **the engine** (`cargo build --release --no-default-features`, copied beside the exe as `kil-engine.exe`) and **the tool pages** (`pnpm build` in `Tools/`, copied to `tools\`). Both are required. Missing tools or outputs fail the release; `build-components.ps1` builds and copies each component once.
6. `-Installer`: downloads the WebView2 Evergreen bootstrapper once into
   `build\redist` and **checks its Microsoft signature**, then runs `makensis`.

## Csproj essentials (`Search\Search.csproj`)

- `net9.0-windows10.0.22621.0`, min `10.0.19041.0`, `WindowsPackageType=None` (unpackaged)
- `WindowsAppSDKSelfContained=true`, `SelfContained=true`
- **Component packages, not the metapackage**: `Microsoft.WindowsAppSDK.WinUI`
  2.3.9, `.Foundation` 2.3.12, `.InteractiveExperiences` 2.1.9, `.DWrite` 2.1.0.
  The metapackage brings ONNX/DirectML/AI (~100 MB nobody needs).
- `Microsoft.Web.WebView2` 1.0.4191.47
- `DISABLE_XAML_GENERATED_MAIN`: our own `Program.Main` handles single instance
- **`PublishResourceIndex` target**: copies `Search.pri` and `*.xbf` into the
  publish folder. **Never remove it.** Without it WinUI can't find its themes
  in release builds (see [[Lessons Learned#The missing resource index]]).

## Output

- `build\Search\`: about 105 MB in the 2026-09-27 verification. Native `Search.exe`, current `kil-engine.exe`, tool pages, WinUI DLLs, `Search.pri`, `App.xbf`, and `Assets\`.
- The folder is portable: copy it anywhere and double-click.

## Installer

`Installer\Search.nsi` (NSIS 3):

- **Per-user**, no admin prompt: installs to `%LOCALAPPDATA%\Programs\Search`
- Requires 64-bit Windows 10 1809 or later
- **WebView2 check**: reads `pv` under `EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}`
  (HKLM, both registry views, and HKCU). If it's missing, runs the bundled
  bootstrapper and checks again. If it's still missing, shows a clear error.
- **WinUI runtime is not installed separately.** It ships inside the app,
  because Microsoft's runtime redistributable is 114.6 MB (see [[Decisions]]).
- Start menu shortcut; registers as a browser (`Software\Clients\StartMenuInternet\Search`,
  `RegisteredApplications`, `SearchURL` ProgID for http/https) so Default apps can pick it
- Apps & features entry with a size, and a quiet uninstall string
- The uninstaller removes the app and its registrations, but **keeps user data**
  (history, bookmarks, passwords, like dragging an app to the Bin on the Mac)
  and keeps WebView2.
- Not code-signed yet, so SmartScreen warns (see [[Roadmap]]).

## Releasing (current manual process)

1. Bump `<Version>` in `Search.csproj`.
2. `.\build.ps1 -Installer -Zip`
3. **Smoke-test the built exe** in a test world (see [[Testing#Release smoke test]]).
4. Commit, then hand over `build\Search-Setup-<ver>-x64.exe`. Installing over the old version keeps user data.

Release output must be an unlinked descendant of the repository. For direct AOT publishing, use "publish-aot.cmd -o build\AOT-check" (quote paths containing spaces); build locally, then copy the finished folder if an external destination is needed. See [[Log/2026-09-26-stabilization]] for the verified fixes and remaining checks.
