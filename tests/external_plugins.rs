//! Tier 2 — externally loaded, sandboxed plugins.
//!
//! Only built with the `external-plugins` feature, which is off by default:
//! the host pulls wasmtime and ~105 transitive crates in, and nobody should pay
//! that on every build for a tier that ships no plugins of its own.
//!
//! The guest modules below are written as WebAssembly *text* so they can be
//! read and reviewed here, rather than committed as binary blobs.
#![cfg(feature = "external-plugins")]

use slicer_engine::gcode::{Move, MoveProgram};
use slicer_engine::plugin::external::load_from;
use slicer_engine::settings::params::SlicingParams;

/// A guest that hands the buffer straight back.
///
/// The whole round trip in its simplest form: host encodes, guest receives it
/// in its own linear memory, host decodes what comes back.
const ECHO_WAT: &str = r#"
(module
  (memory (export "memory") 16)
  (func (export "plugin_abi_version") (result i32) i32.const 1)
  (func (export "plugin_alloc") (param i32) (result i32) i32.const 1024)
  (func (export "plugin_filter_moves") (param i32 i32) (result i64)
    local.get 0 i64.extend_i32_u (i64.const 32) i64.shl
    local.get 1 i64.extend_i32_u
    i64.or))
"#;

/// A guest that drops the program's last record by decrementing the count in
/// the header and returning a shorter buffer.
const DROP_LAST_WAT: &str = r#"
(module
  (memory (export "memory") 16)
  (func (export "plugin_abi_version") (result i32) i32.const 1)
  (func (export "plugin_alloc") (param i32) (result i32) i32.const 1024)
  (func (export "plugin_filter_moves") (param i32 i32) (result i64)
    ;; header count lives at ptr+8
    (i32.store
      (i32.add (local.get 0) (i32.const 8))
      (i32.sub (i32.load (i32.add (local.get 0) (i32.const 8))) (i32.const 1)))
    local.get 0 i64.extend_i32_u (i64.const 32) i64.shl
    (i64.extend_i32_u (i32.sub (local.get 1) (i32.const 72)))
    i64.or))
"#;

/// A guest that returns a buffer outside its own memory.
const OUT_OF_BOUNDS_WAT: &str = r#"
(module
  (memory (export "memory") 1)
  (func (export "plugin_abi_version") (result i32) i32.const 1)
  (func (export "plugin_alloc") (param i32) (result i32) i32.const 1024)
  (func (export "plugin_filter_moves") (param i32 i32) (result i64)
    (i64.or
      (i64.shl (i64.const 4000000) (i64.const 32))
      (i64.const 1000000))))
"#;

/// A guest that never returns.
const SPIN_WAT: &str = r#"
(module
  (memory (export "memory") 1)
  (func (export "plugin_abi_version") (result i32) i32.const 1)
  (func (export "plugin_alloc") (param i32) (result i32) i32.const 1024)
  (func (export "plugin_filter_moves") (param i32 i32) (result i64)
    (loop (br 0))
    i64.const 0))
"#;

/// A module built against a different ABI.
const WRONG_ABI_WAT: &str = r#"
(module
  (memory (export "memory") 1)
  (func (export "plugin_abi_version") (result i32) i32.const 99)
  (func (export "plugin_alloc") (param i32) (result i32) i32.const 1024)
  (func (export "plugin_filter_moves") (param i32 i32) (result i64) i64.const 0))
"#;

/// A module missing a required export.
const INCOMPLETE_WAT: &str = r#"
(module
  (memory (export "memory") 1)
  (func (export "plugin_abi_version") (result i32) i32.const 1))
"#;

fn plugin_dir(modules: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    for (name, wat) in modules {
        let bytes = wat::parse_str(wat).expect("valid wat");
        std::fs::write(dir.path().join(format!("{name}.wasm")), bytes).expect("write module");
    }
    dir
}

fn sample_program() -> MoveProgram {
    let mut p = MoveProgram::new();
    p.raw(";TYPE:Outer wall\n");
    p.push(Move::Travel {
        x: 1.0,
        y: 2.0,
        feed_mm_min: 9000.0,
        comment: Some("travel".into()),
    });
    p.push(Move::Extrude {
        x: 3.0,
        y: 4.0,
        z: None,
        e: 0.5,
        de: 0.5,
        feed_mm_min: 1800.0,
        role: slicer_engine::core::ExtrusionRole::OuterWall,
        width_mm: 0.4,
        comment: None,
    });
    p
}

#[test]
fn a_module_round_trips_the_program_unchanged() {
    let dir = plugin_dir(&[("echo", ECHO_WAT)]);
    let loaded = load_from(dir.path());
    assert!(loaded.failures.is_empty(), "{:?}", loaded.failures);
    assert_eq!(loaded.plugins.len(), 1);

    let filters = slicer_engine::plugin::move_filters(&loaded.plugins);
    assert_eq!(
        filters.len(),
        1,
        "the loader adapts a module to the Tier 1 hook"
    );

    let before = sample_program();
    let mut program = before.clone();
    filters[0].filter(&mut program, &SlicingParams::default());
    assert_eq!(before.moves(), program.moves());
}

#[test]
fn a_module_can_actually_rewrite_the_program() {
    let dir = plugin_dir(&[("drop-last", DROP_LAST_WAT)]);
    let loaded = load_from(dir.path());
    assert!(loaded.failures.is_empty(), "{:?}", loaded.failures);

    let filters = slicer_engine::plugin::move_filters(&loaded.plugins);
    let mut program = sample_program();
    let before = program.len();
    filters[0].filter(&mut program, &SlicingParams::default());
    assert_eq!(program.len(), before - 1, "the guest's rewrite must land");
}

#[test]
fn a_module_reaching_outside_its_memory_is_refused() {
    // The host must not take a guest's word for where its output lives.
    let dir = plugin_dir(&[("oob", OUT_OF_BOUNDS_WAT)]);
    let loaded = load_from(dir.path());
    let filters = slicer_engine::plugin::move_filters(&loaded.plugins);

    let before = sample_program();
    let mut program = before.clone();
    filters[0].filter(&mut program, &SlicingParams::default());
    assert_eq!(
        before.moves(),
        program.moves(),
        "a refused module costs its own feature, never the print"
    );
}

#[test]
fn a_module_that_never_returns_does_not_hang_the_slice() {
    let dir = plugin_dir(&[("spin", SPIN_WAT)]);
    let loaded = load_from(dir.path());
    let filters = slicer_engine::plugin::move_filters(&loaded.plugins);

    let before = sample_program();
    let mut program = before.clone();
    let started = std::time::Instant::now();
    filters[0].filter(&mut program, &SlicingParams::default());
    assert!(
        started.elapsed() < std::time::Duration::from_secs(90),
        "the epoch interrupt must cut a runaway module off"
    );
    assert_eq!(before.moves(), program.moves());
}

#[test]
fn a_module_from_a_different_abi_is_not_loaded() {
    let dir = plugin_dir(&[("future", WRONG_ABI_WAT)]);
    let loaded = load_from(dir.path());
    assert!(loaded.plugins.is_empty());
    assert_eq!(loaded.failures.len(), 1, "and the refusal is reported");
}

#[test]
fn a_module_missing_an_export_is_not_loaded() {
    let dir = plugin_dir(&[("incomplete", INCOMPLETE_WAT)]);
    let loaded = load_from(dir.path());
    assert!(loaded.plugins.is_empty());
    assert_eq!(loaded.failures.len(), 1);
}

#[test]
fn one_bad_module_does_not_stop_the_others() {
    let dir = plugin_dir(&[("echo", ECHO_WAT), ("incomplete", INCOMPLETE_WAT)]);
    let loaded = load_from(dir.path());
    assert_eq!(loaded.plugins.len(), 1);
    assert_eq!(loaded.failures.len(), 1);
}

#[test]
fn a_missing_plugin_directory_is_not_an_error() {
    // Most installations have no plugins at all.
    let loaded = load_from(std::path::Path::new("/nonexistent/plugins"));
    assert!(loaded.plugins.is_empty());
    assert!(loaded.failures.is_empty());
}

#[test]
fn modules_load_in_a_deterministic_order() {
    // Filters compose, so the order they run in must not depend on the
    // filesystem's iteration order.
    let dir = plugin_dir(&[("b-echo", ECHO_WAT), ("a-echo", ECHO_WAT)]);
    let loaded = load_from(dir.path());
    let ids: Vec<&str> = loaded.plugins.iter().map(|p| p.manifest().id).collect();
    assert_eq!(ids, vec!["a-echo", "b-echo"]);
}
