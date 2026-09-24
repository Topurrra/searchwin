# tesseract-runtime

Bundles a working Tesseract 5.5+ OCR engine into the KeepItLocal installer
so end users get OCR without installing Tesseract separately.

This folder is **gitignored for binaries** but the README is tracked, so
fresh clones see these setup instructions.

## Current contents (as set up by Wave 8.4)

19 files, ~16 MB, from **vcpkg's `tesseract:x64-windows` install**
(Tesseract 5.5.2 + leptonica 1.87.0, built via vcpkg from source):

```
tesseract-runtime/
├── tesseract.exe                  ← subprocess OCR (existing path)
├── tesseract55.dll                ← libtesseract for leptess in-process OCR
├── leptonica-1.87.0.dll           ← image library libtesseract depends on
├── archive.dll                    ← libarchive (PDF input)
├── bz2.dll                        ← compression
├── gif.dll                        ← GIF format
├── jpeg62.dll                     ← JPEG format
├── libcrypto-3-x64.dll            ← OpenSSL (libcurl dep)
├── libcurl.dll                    ← curl (tesseract optional feature)
├── liblzma.dll                    ← LZMA compression
├── libpng16.dll                   ← PNG format
├── libsharpyuv.dll                ← libwebp dep
├── libwebp.dll                    ← WebP format
├── libwebpmux.dll                 ← WebP multiplex
├── lz4.dll                        ← LZ4 compression
├── openjp2.dll                    ← JPEG 2000
├── tiff.dll                       ← TIFF format
├── z.dll                          ← zlib
└── zstd.dll                       ← Zstandard compression
```

Mirrored in `tauri.conf.json bundle.resources` so the installer ships them.

## How this was set up (one time, on this machine)

For future references and other devs cloning this repo:

```powershell
# 1. Clone + bootstrap vcpkg (one-time, ~1 min)
git clone https://github.com/microsoft/vcpkg.git C:\vcpkg
C:\vcpkg\bootstrap-vcpkg.bat

# 2. Install Tesseract via vcpkg (~9 min build-from-source)
C:\vcpkg\vcpkg.exe install tesseract:x64-windows --disable-metrics

# 3. Set build-script env vars persistently (open new shell after this)
setx VCPKG_ROOT "C:\vcpkg"
setx VCPKGRS_TRIPLET "x64-windows"
setx VCPKGRS_DYNAMIC "1"
setx TESSERACT_INCLUDE_PATHS "C:\vcpkg\installed\x64-windows\include"
setx TESSERACT_LINK_PATHS    "C:\vcpkg\installed\x64-windows\lib"
setx LEPTONICA_INCLUDE_PATH  "C:\vcpkg\installed\x64-windows\include\leptonica"
setx LEPTONICA_LINK_PATHS    "C:\vcpkg\installed\x64-windows\lib"

# 4. Copy the runtime files from vcpkg into this folder
cp C:\vcpkg\installed\x64-windows\tools\tesseract\*.exe  src-tauri\tesseract-runtime\
cp C:\vcpkg\installed\x64-windows\tools\tesseract\*.dll  src-tauri\tesseract-runtime\
```

## Why vcpkg (not UB-Mannheim's installer)?

UB-Mannheim's installer ships **only the runtime DLLs** by default. To
build `leptess` (in-process libtesseract OCR), we need the dev headers +
`.lib` files too — and those need to MATCH the runtime DLLs.

vcpkg builds both together from one source: headers, `.lib`, and DLLs all
align. Trying to mix UB-Mannheim runtime DLLs with vcpkg `.lib` files
(or vice versa) is a recipe for symbol mismatches.

## How OCR resolves at runtime

`commands/ocr.rs::resolve_tesseract()` checks in this order:
1. `tesseract.exe` next to the running `keepitlocal.exe` (this bundle).
2. System PATH `tesseract`.
3. Common Windows install locations (`C:\Program Files\Tesseract-OCR\`,
   `C:\Program Files\tesseract.exe`, etc.).

So the bundled exe wins automatically — end users never need a system
Tesseract.

With `--features ocr-leptess`, `ocr_image_file` ALSO routes index-side
OCR calls through the in-process libtesseract loaded from `tesseract55.dll`
(same DLL the subprocess path also depends on transitively).

## Building with leptess

```powershell
cargo build --features ocr-leptess
```

Without the feature: subprocess path. With it: in-process path for
non-cancellable, non-preprocessed OCR calls (i.e., the index-side
ones); cancellable / preprocessed calls (the OCR tool UI) still use
subprocess.
