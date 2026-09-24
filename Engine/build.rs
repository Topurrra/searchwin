// Build script: the (optional) libvosk linker hookup.
//
// The libvosk hookup is conditional on the `vosk` Cargo feature so the
// project keeps building cleanly on machines that don't have the engine
// installed. When the feature is on we expect:
//
//   1. Env var `VOSK_LIB_DIR` pointing at the folder that contains
//      libvosk.dll, libvosk.lib, and vosk_api.h. (Same folder you got
//      from extracting vosk-win64-0.3.45.zip.)
//   2. The libvosk.dll alongside the final .exe at runtime, OR on PATH.
//      We don't try to embed it — the user (or installer) is responsible
//      for placing it.
//
// Fallback: if VOSK_LIB_DIR isn't set, we emit a build-time hard error
// so the user knows immediately what's missing (rather than a cryptic
// link error 5 minutes into the build).

fn main() {
    #[cfg(feature = "vosk")]
    wire_vosk();
}

#[cfg(feature = "vosk")]
fn wire_vosk() {
    use std::env;
    use std::path::PathBuf;

    println!("cargo:rerun-if-env-changed=VOSK_LIB_DIR");

    let vosk_dir = env::var("VOSK_LIB_DIR").unwrap_or_else(|_| {
        panic!(
            "The `vosk` feature is enabled but VOSK_LIB_DIR is not set. \
             Set it to the folder containing libvosk.dll + libvosk.lib + \
             vosk_api.h (the contents of vosk-win64-0.3.45.zip extracted), \
             then rebuild. Example: setx VOSK_LIB_DIR \"C:\\vosk-engine\""
        )
    });

    let dir = PathBuf::from(&vosk_dir);
    if !dir.exists() {
        panic!(
            "VOSK_LIB_DIR points to {vosk_dir:?} which doesn't exist. \
             Either fix the env var or unset it and rebuild without --features vosk."
        );
    }

    // Tell rustc where to find libvosk.lib at link time.
    println!("cargo:rustc-link-search=native={}", dir.display());

    // Tell rustc to link the import library. The prebuilt zip ships
    // the file as `libvosk.lib` literally — that's the full file name,
    // not a Unix-style "lib" prefix. On MSVC the linker takes the name
    // verbatim and appends `.lib`, so we ask for `libvosk` here, which
    // resolves to `libvosk.lib` in VOSK_LIB_DIR. Runtime dependency is
    // libvosk.dll (must be alongside the .exe or on PATH).
    println!("cargo:rustc-link-lib=dylib=libvosk");
}
