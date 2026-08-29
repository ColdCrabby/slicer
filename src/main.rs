// The CLI binary only exists on host targets. `slicer_engine::cli` is compiled
// away for wasm32 (browser builds link the library through wasm-bindgen) and
// for iOS (the app links the library into the Tauri shell), so the binary would
// not resolve there. A stub `main` keeps a whole-package `cargo check` against
// those targets honest instead of failing on an unbuildable bin target.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "ios")))]
fn main() {
    use slicer_engine::cli::CliArgs;

    if let Err(e) = CliArgs::run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

#[cfg(any(target_arch = "wasm32", target_os = "ios"))]
fn main() {}
