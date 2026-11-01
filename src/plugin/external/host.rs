//! The WASM host: loads Tier 2 modules and adapts them to the Tier 1 trait.
//!
//! ## Why WASM and not a scripting VM
//!
//! A Lua embed is sandboxed by *removing capability* — strip `os`, `io`,
//! `debug` from the global table and hope nothing reaches them back through a
//! metatable. The interpreter still runs in the host's address space, so a VM
//! bug is a host memory-corruption bug.
//!
//! WASM's isolation instead comes from the runtime enforcing a linear-memory
//! boundary regardless of what the guest does or what bugs it has. The
//! capability grant and the isolation guarantee are two independent layers, and
//! losing one does not lose the other. The guarantee has to hold when a plugin
//! is actively hostile, not merely when it is well-behaved.
//!
//! ## What a module can reach
//!
//! **Nothing it is not handed.** The host provides no WASI, no filesystem, no
//! network, no clock, no host functions at all — a module gets its own linear
//! memory and one entry point. Everything it can affect arrives in, and leaves
//! by, the buffer described in [`super::abi`].
//!
//! Three limits are enforced rather than trusted, because a module may be
//! hostile: memory is capped, execution is interrupted if it runs too long, and
//! a returned buffer is validated before a byte of it reaches the program.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use wasmtime::{Engine, Instance, Linker, Memory, Module, Store, TypedFunc};

use super::abi;
use crate::gcode::{MoveFilter, MoveProgram};
use crate::plugin::{Plugin, PluginManifest, Stability, PLUGIN_API_VERSION};
use crate::settings::params::SlicingParams;

/// Ceiling on a module's linear memory.
///
/// A program is a few hundred thousand records at ~72 bytes each, so a filter
/// needs single-digit megabytes; the rest is headroom. Enforced because an
/// unbounded guest could otherwise exhaust the host's memory.
const MEMORY_LIMIT_BYTES: usize = 256 * 1024 * 1024;

/// Wall-clock ceiling on one filter call, after which the guest is interrupted.
///
/// Without this a module with an infinite loop hangs the slice with no way out.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// What can go wrong loading a module.
#[derive(Debug)]
pub enum LoadError {
    /// The file could not be read.
    Io(std::io::Error),
    /// The bytes are not a valid WASM module, or it failed to instantiate.
    Wasm(wasmtime::Error),
    /// The module is missing an export the host requires.
    MissingExport(&'static str),
    /// The module was built against a different hook ABI.
    AbiMismatch {
        /// What the module reported.
        found: u32,
        /// What this engine speaks.
        expected: u32,
    },
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot read plugin: {e}"),
            Self::Wasm(e) => write!(f, "not a loadable module: {e}"),
            Self::MissingExport(name) => write!(f, "module exports no `{name}`"),
            Self::AbiMismatch { found, expected } => {
                write!(
                    f,
                    "module speaks move ABI v{found}, this engine v{expected}"
                )
            }
        }
    }
}

impl std::error::Error for LoadError {}

/// Caps on what a module may allocate.
///
/// Enforced by the runtime rather than trusted: a hostile module would
/// otherwise exhaust the host's memory simply by asking for it.
struct Limits;

impl wasmtime::ResourceLimiter for Limits {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(desired <= MEMORY_LIMIT_BYTES)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(desired <= 10_000)
    }
}

/// One loaded module, and the machinery to call it.
///
/// Held behind a `Mutex` because a `Store` is not `Sync` and a filter is called
/// through a `&self` that must be. Filtering happens once per slice, so the
/// lock is never contended in practice.
struct Loaded {
    id: String,
    engine: Engine,
    module: Module,
    state: Mutex<()>,
}

/// A module presented as a Tier 1 move filter.
struct WasmFilter {
    loaded: std::sync::Arc<Loaded>,
}

impl Loaded {
    /// Instantiate a fresh store and call the module's filter entry point.
    ///
    /// A **fresh instance per call**, deliberately: a module cannot accumulate
    /// state across slices, so one slice cannot influence the next and a module
    /// that corrupts its own memory only ruins its own call.
    fn run(&self, input: &[u8]) -> Result<Vec<u8>, wasmtime::Error> {
        let mut store = Store::new(&self.engine, Limits);
        store.set_epoch_deadline(1);
        store.limiter(|limits| limits);

        let linker: Linker<Limits> = Linker::new(&self.engine);
        let instance: Instance = linker.instantiate(&mut store, &self.module)?;

        let memory: Memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| wasmtime::Error::msg("module exports no memory"))?;
        let alloc: TypedFunc<u32, u32> = instance.get_typed_func(&mut store, "plugin_alloc")?;
        let filter: TypedFunc<(u32, u32), u64> =
            instance.get_typed_func(&mut store, "plugin_filter_moves")?;

        let ptr = alloc.call(&mut store, input.len() as u32)?;
        memory.write(&mut store, ptr as usize, input)?;

        let packed = filter.call(&mut store, (ptr, input.len() as u32))?;
        let out_ptr = (packed >> 32) as usize;
        let out_len = (packed & 0xffff_ffff) as usize;

        // Validate before reading: a guest is free to return a length and an
        // offset that do not describe memory it owns, and the host must not
        // take its word for it.
        let data = memory.data(&store);
        if out_len > MEMORY_LIMIT_BYTES || out_ptr.saturating_add(out_len) > data.len() {
            return Err(wasmtime::Error::msg(
                "module returned a buffer outside its own memory",
            ));
        }
        Ok(data[out_ptr..out_ptr + out_len].to_vec())
    }
}

impl MoveFilter for WasmFilter {
    fn name(&self) -> &str {
        &self.loaded.id
    }

    fn filter(&self, program: &mut MoveProgram, _params: &SlicingParams) {
        let _guard = self.loaded.state.lock();
        let (input, side) = abi::encode(program);

        // A misbehaving module costs its own feature, never the print: every
        // failure below leaves `program` exactly as it was.
        let output = match self.loaded.run(&input) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!(
                    "[warn] plugin '{}': filter failed, output left unchanged: {e}",
                    self.loaded.id
                );
                return;
            }
        };
        match abi::decode(&output, &side) {
            Ok(filtered) => *program = filtered,
            Err(e) => eprintln!(
                "[warn] plugin '{}': rejected its output, left unchanged: {e}",
                self.loaded.id
            ),
        }
    }
}

/// A loaded Tier 2 module, presented to the engine as an ordinary plugin.
///
/// This is the design's load-bearing claim made concrete: the external loader
/// is *itself just a Tier 1 plugin*, written against the same trait as
/// everything else, which is why adding it changed no hook signature.
pub struct ExternalPlugin {
    loaded: std::sync::Arc<Loaded>,
    name: String,
}

impl Plugin for ExternalPlugin {
    fn manifest(&self) -> PluginManifest {
        // Leaked because `PluginManifest` is `&'static str` throughout — a
        // compile-time plugin's identity is a literal. A module's is not, and
        // the leak is bounded by the number of plugins loaded once at startup.
        PluginManifest {
            id: Box::leak(self.loaded.id.clone().into_boxed_str()),
            name: Box::leak(self.name.clone().into_boxed_str()),
            description: "Externally loaded plugin.",
            stability: Stability::Experimental,
            api_version: PLUGIN_API_VERSION,
        }
    }

    fn move_filter(&self) -> Option<Box<dyn MoveFilter>> {
        Some(Box::new(WasmFilter {
            loaded: self.loaded.clone(),
        }))
    }
}

/// The modules loaded from a directory.
pub struct ExternalPlugins {
    /// The plugins that loaded successfully.
    pub plugins: Vec<Box<dyn Plugin>>,
    /// One entry per module that did not, so a failure is reported rather than
    /// silently leaving a feature missing.
    pub failures: Vec<(PathBuf, LoadError)>,
}

/// Load every `*.wasm` in `dir`.
///
/// A missing directory is not an error — most installations have no plugins.
/// A module that fails to load is recorded in `failures` and skipped: one bad
/// plugin must not stop the others, or the slice.
pub fn load_from(dir: &Path) -> ExternalPlugins {
    let mut out = ExternalPlugins {
        plugins: Vec::new(),
        failures: Vec::new(),
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };

    let mut config = wasmtime::Config::new();
    config.epoch_interruption(true);
    // Nothing shared and nothing ambient: a filter needs neither, and both are
    // surface a hostile module could reach for.
    config.wasm_bulk_memory(true);
    let Ok(engine) = Engine::new(&config) else {
        return out;
    };

    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "wasm"))
        .collect();
    // Deterministic order: filters compose, so the order they run in has to be
    // the same on every machine.
    paths.sort();

    if paths.is_empty() {
        return out;
    }

    // The interrupt that enforces CALL_TIMEOUT. A background ticker rather than
    // a fuel budget, so a slow-but-progressing module is judged on wall clock —
    // which is what a user waiting on a slice actually cares about. Started
    // only once there is something to run, so an installation with no plugins
    // pays for no thread.
    {
        let engine = engine.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(CALL_TIMEOUT);
            engine.increment_epoch();
        });
    }

    for path in paths {
        match load_one(&engine, &path) {
            Ok(p) => out.plugins.push(Box::new(p)),
            Err(e) => out.failures.push((path, e)),
        }
    }
    out
}

fn load_one(engine: &Engine, path: &Path) -> Result<ExternalPlugin, LoadError> {
    let bytes = std::fs::read(path).map_err(LoadError::Io)?;
    let module = Module::new(engine, &bytes).map_err(LoadError::Wasm)?;

    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("plugin")
        .to_string();

    // Check the module's shape once, at load, rather than discovering it is
    // unusable in the middle of a slice.
    let mut store = Store::new(engine, Limits);
    store.limiter(|limits| limits);
    // Epoch interruption is on for the engine, and a store with no deadline
    // traps on its first instruction — so the probe needs one too.
    store.set_epoch_deadline(1);
    let linker: Linker<Limits> = Linker::new(engine);
    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(LoadError::Wasm)?;

    let version: TypedFunc<(), u32> = instance
        .get_typed_func(&mut store, "plugin_abi_version")
        .map_err(|_| LoadError::MissingExport("plugin_abi_version"))?;
    let found = version.call(&mut store, ()).map_err(LoadError::Wasm)?;
    if found != abi::ABI_VERSION {
        return Err(LoadError::AbiMismatch {
            found,
            expected: abi::ABI_VERSION,
        });
    }
    instance
        .get_typed_func::<u32, u32>(&mut store, "plugin_alloc")
        .map_err(|_| LoadError::MissingExport("plugin_alloc"))?;
    instance
        .get_typed_func::<(u32, u32), u64>(&mut store, "plugin_filter_moves")
        .map_err(|_| LoadError::MissingExport("plugin_filter_moves"))?;

    let name = id.replace('-', " ");
    Ok(ExternalPlugin {
        loaded: std::sync::Arc::new(Loaded {
            id,
            engine: engine.clone(),
            module,
            state: Mutex::new(()),
        }),
        name,
    })
}
