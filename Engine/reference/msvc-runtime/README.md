# msvc-runtime

The **Microsoft Visual C++ runtime** DLLs bundled alongside KeepItLocal's
native binaries. KeepItLocal is an MSVC build and the bundled native exes/DLLs
(`tesseract.exe` + its image DLLs, `pdfium.dll`, `libvosk.dll`) are linked
against this runtime; shipping the exact versions guarantees they load
regardless of the target machine's installed VC++ redistributable.

> History: this folder was previously `qpdf-runtime` and also held `qpdf.exe` +
> `qpdf30.dll` for the (now-removed) Workspace PDF pack. The PDF tools migrated
> to KeepItLocal Privacy and the qpdf binaries were dropped (~6.8 MB reclaimed);
> the shared VC++ runtime stays because `tesseract.exe` and the other native
> binaries depend on it.

## Files

| File | Purpose |
|------|---------|
| `vcruntime140.dll`, `vcruntime140_1.dll` | MSVC C runtime |
| `msvcp140.dll`, `msvcp140_1.dll`, `msvcp140_2.dll`, `msvcp140_atomic_wait.dll`, `msvcp140_codecvt_ids.dll` | MSVC C++ runtime |
| `concrt140.dll` | MSVC concurrency runtime |

Bundled at the install root (next to the native exes) via
`tauri.conf.json` → `bundle.resources`.
