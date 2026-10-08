//! The plugins compiled into this build.
//!
//! Two things live here, and they are not the same kind of thing:
//!
//! - **Internal plugins**, installed by the entry point that needs them rather
//!   than being always-on. [`DebugCapture`] is the only one today, and it is
//!   what turned the debug pipeline from a second copy of `process_mesh` into
//!   an ordinary set of stages.
//! - **Experiments** — optional, opinionated features shipped in every build
//!   and off by default, listed by [`crate::plugin::builtin_plugins`].
//!   [`HelloWorld`] is the worked example; [`ArcFitting`] is the first real
//!   feature, and the one the move-filter hook was built for.

mod arc_fitting;
#[cfg(not(target_arch = "wasm32"))]
mod debug_capture;
mod hello_world;

pub use arc_fitting::ArcFitting;
#[cfg(not(target_arch = "wasm32"))]
pub use debug_capture::DebugCapture;
pub use hello_world::HelloWorld;
