#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Desktop launcher. All application wiring lives in the library crate so that
// mobile targets — which have no Rust `main` and instead link this crate as a
// static library — run the exact same code. See `src/lib.rs`.
fn main() {
    slicer_ui_desktop_lib::run()
}
