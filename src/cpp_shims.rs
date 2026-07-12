//! C++ ABI shims for `wasm32-unknown-unknown`.
//!
//! When Clipper2's C++ code is compiled by the WASI SDK clang++, the resulting
//! object files reference libc++ `operator new` / `operator delete` and
//! `__libcpp_verbose_abort` under their Itanium-mangled names. These are
//! normally resolved by linking against `libc++.a`, but we omit that archive
//! for the WASM target (`CXXSTDLIB_wasm32_unknown_unknown = ""`). Without
//! definitions, wasm-bindgen emits `import * from "env"` entries that Vite
//! cannot resolve.
//!
//! This module provides thin Rust implementations under the exact mangled
//! names so the linker resolves every reference internally. The resulting
//! WASM binary has no `env` module imports.
//!
//! ## Allocation strategy
//!
//! A fixed 8-byte header is prepended to every allocation. It stores the
//! original request size as a `u32` (sufficient for wasm32 which is limited
//! to 4 GiB). This lets the unsized `operator delete(void*)` reconstruct the
//! [`Layout`] without external bookkeeping.
//!
//! Alignment is fixed at 8 bytes — suitable for all Clipper2 types (`i64`,
//! `f64`, pointer-sized structs). Higher-alignment allocations are not needed
//! by this library.

use std::alloc::{alloc, dealloc, Layout};

/// Size of the bookkeeping header prepended to every allocation (must equal
/// `ALIGN` so the data pointer is correctly aligned).
const HEADER: usize = 8;
/// Minimum alignment for all C++ heap allocations.
const ALIGN: usize = 8;
/// Sentinel returned for zero-size allocations (non-null, never freed).
const ZERO_SIZE_SENTINEL: usize = ALIGN;

/// Allocate `size` bytes with an 8-byte header storing the original size.
/// Returns a non-null sentinel for zero-size requests.
#[inline]
unsafe fn cpp_alloc(size: usize) -> *mut u8 {
    if size == 0 {
        return ZERO_SIZE_SENTINEL as *mut u8;
    }
    let total = size + HEADER;
    let layout = Layout::from_size_align_unchecked(total, ALIGN);
    let base = alloc(layout);
    if base.is_null() {
        return base;
    }
    *(base as *mut u32) = size as u32;
    base.add(HEADER)
}

/// Free a pointer allocated by [`cpp_alloc`]; reads size from the header.
#[inline]
unsafe fn cpp_dealloc_unsized(ptr: *mut u8) {
    if ptr.is_null() || ptr as usize == ZERO_SIZE_SENTINEL {
        return;
    }
    let base = ptr.sub(HEADER);
    let size = *(base as *const u32) as usize;
    let layout = Layout::from_size_align_unchecked(size + HEADER, ALIGN);
    dealloc(base, layout);
}

/// Free a pointer with caller-supplied size (from `operator delete(void*, size_t)`).
#[inline]
unsafe fn cpp_dealloc_sized(ptr: *mut u8, size: usize) {
    if ptr.is_null() || ptr as usize == ZERO_SIZE_SENTINEL {
        return;
    }
    let base = ptr.sub(HEADER);
    let layout = Layout::from_size_align_unchecked(size + HEADER, ALIGN);
    dealloc(base, layout);
}

// ─── operator new(size_t) ─────────────────────────────────────────────────
// WASM type: (i32) -> (i32)
#[export_name = "_Znwm"]
pub unsafe extern "C" fn cpp_operator_new(size: usize) -> *mut u8 {
    cpp_alloc(size)
}

// ─── operator new[](size_t) ───────────────────────────────────────────────
// WASM type: (i32) -> (i32)
#[export_name = "_Znam"]
pub unsafe extern "C" fn cpp_operator_new_array(size: usize) -> *mut u8 {
    cpp_alloc(size)
}

// ─── operator new(size_t, std::nothrow_t const&) ──────────────────────────
// WASM type: (i32, i32) -> (i32)
#[export_name = "_ZnwmRKSt9nothrow_t"]
pub unsafe extern "C" fn cpp_operator_new_nothrow(size: usize, _nothrow: usize) -> *mut u8 {
    cpp_alloc(size)
}

// ─── operator delete(void*) ───────────────────────────────────────────────
// WASM type: (i32) -> ()
#[export_name = "_ZdlPv"]
pub unsafe extern "C" fn cpp_operator_delete(ptr: *mut u8) {
    cpp_dealloc_unsized(ptr);
}

// ─── operator delete[](void*) ─────────────────────────────────────────────
// WASM type: (i32) -> ()
#[export_name = "_ZdaPv"]
pub unsafe extern "C" fn cpp_operator_delete_array(ptr: *mut u8) {
    cpp_dealloc_unsized(ptr);
}

// ─── operator delete(void*, size_t) ───────────────────────────────────────
// WASM type: (i32, i32) -> ()
#[export_name = "_ZdlPvm"]
pub unsafe extern "C" fn cpp_operator_delete_sized(ptr: *mut u8, size: usize) {
    cpp_dealloc_sized(ptr, size);
}

// ─── std::__libcpp_verbose_abort(char const*, ...) ────────────────────────
// Called by libc++ on assertion failures; must not return.
// WASM type: (i32, i32) -> ()  — clang WASM variadic ABI passes the va_list
// pointer as a second i32 parameter.
#[export_name = "_ZNSt3__222__libcpp_verbose_abortEPKcz"]
pub unsafe extern "C" fn cpp_libcpp_verbose_abort(_fmt: *const u8, _va: usize) {
    core::arch::wasm32::unreachable();
}

// ─── std::nothrow ─────────────────────────────────────────────────────────
// `std::nothrow` is a global tag object of type `std::nothrow_t` used as the
// second argument to `operator new(size_t, std::nothrow_t const&)`.  It is an
// empty struct so any static byte serves as a valid definition.  Without this
// symbol the linker fails with "undefined symbol: _ZSt7nothrow" when Clipper2
// calls `new (std::nothrow) T(…)`.
#[no_mangle]
pub static _ZSt7nothrow: u8 = 0;
