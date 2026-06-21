// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Structurally-safe FFI free combinators.
//!
//! These functions enforce the correct sequence for freeing opaque FFI handles:
//!
//! 1. Null check (early return)
//! 2. Panic catch (unwind boundary)
//! 3. `mark_freed` via the selected [`FfiTracker`] (BEFORE deallocation — tracks pointer while live)
//! 4. Optional teardown closure (runs while the struct is still live)
//! 5. `Box::from_raw` + drop (actual deallocation)
//!
//! This ordering is critical: `mark_freed` MUST happen before `Box::from_raw`
//! so the pointer is tracked while the memory is still valid. The previous
//! pattern delegated tracking to caller closures, creating TOCTOU bugs when
//! callers called `mark_freed` after `Box::from_raw` (pointer already dangling).
//!
//! Part of #6577.

use crate::FfiErrorCode;
use crate::verification::FfiTracker;
use core::ffi::c_void;
use std::ffi::CString;

/// Free a `Box`-allocated FFI handle (V2 — returns error code).
///
/// Sequence: null check → panic catch → `mark_freed` → `Box::from_raw` + drop.
///
/// Returns `E::double_free()` if the pointer was already freed.
///
/// # Safety
///
/// - `handle` must be null or a valid pointer from `Box::into_raw`.
/// - `handle` must not have been freed previously (detected, not UB).
pub unsafe fn box_handle_free_v2<H, E: FfiErrorCode>(
    name: &'static str,
    handle: *mut H,
    tracker: FfiTracker,
) -> E {
    crate::aterm_ffi_catch_unwind!(E::internal(), { /* panic caught at FFI boundary */ }, {
        if handle.is_null() {
            return E::null_handle();
        }
        if tracker.is_freed(handle.cast::<c_void>()) {
            return E::double_free();
        }
        if !tracker.is_allocated(handle.cast::<c_void>()) {
            aterm_log::error!("{}: rejecting free of untracked handle {:p}", name, handle);
            return E::internal();
        }
        tracker.mark_freed(handle.cast::<c_void>());
        // SAFETY: Caller guarantees handle is valid pointer from Box::into_raw.
        drop(unsafe { Box::from_raw(handle) });
        E::ok()
    })
}

/// Free a `Box`-allocated FFI handle with pre-free teardown (V2 — returns error code).
///
/// Sequence: null check → panic catch → `mark_freed` → teardown → `Box::from_raw` + drop.
///
/// The `teardown` closure receives the raw pointer (guaranteed non-null, not yet
/// deallocated) and can perform cleanup such as disarming callback guards or
/// clearing magic canaries. The struct is still live during teardown.
///
/// Returns `E::double_free()` if the pointer was already freed.
///
/// # Safety
///
/// - `handle` must be null or a valid pointer from `Box::into_raw`.
/// - `handle` must not have been freed previously (detected, not UB).
/// - The `teardown` closure must not deallocate `handle`.
pub unsafe fn box_handle_free_v2_with_teardown<H, E, F>(
    name: &'static str,
    handle: *mut H,
    tracker: FfiTracker,
    teardown: F,
) -> E
where
    E: FfiErrorCode,
    F: FnOnce(*mut H),
{
    crate::aterm_ffi_catch_unwind!(E::internal(), { /* panic caught at FFI boundary */ }, {
        if handle.is_null() {
            return E::null_handle();
        }
        if tracker.is_freed(handle.cast::<c_void>()) {
            return E::double_free();
        }
        if !tracker.is_allocated(handle.cast::<c_void>()) {
            aterm_log::error!("{}: rejecting free of untracked handle {:p}", name, handle);
            return E::internal();
        }
        tracker.mark_freed(handle.cast::<c_void>());
        teardown(handle);
        // SAFETY: Caller guarantees handle is valid pointer from Box::into_raw.
        drop(unsafe { Box::from_raw(handle) });
        E::ok()
    })
}

/// Free a `Box`-allocated FFI handle (V1 — void return).
///
/// Sequence: null check → panic catch → `assert_not_freed` → `mark_freed` → `Box::from_raw` + drop.
///
/// Panics (caught by unwind guard) on double-free via `assert_not_freed`.
///
/// # Safety
///
/// - `handle` must be null or a valid pointer from `Box::into_raw`.
/// - `handle` must not have been freed previously (panics on double-free).
pub unsafe fn box_handle_free_v1<H>(name: &'static str, handle: *mut H, tracker: FfiTracker) {
    crate::aterm_ffi_catch_unwind!((), { /* panic caught at FFI boundary */ }, {
        if handle.is_null() {
            return;
        }
        if tracker.is_freed(handle.cast::<c_void>()) {
            tracker.assert_not_freed(handle.cast::<c_void>());
        }
        if !tracker.is_allocated(handle.cast::<c_void>()) {
            aterm_log::error!("{}: rejecting free of untracked handle {:p}", name, handle);
            return;
        }
        tracker.assert_not_freed(handle.cast::<c_void>());
        tracker.mark_freed(handle.cast::<c_void>());
        // SAFETY: Caller guarantees handle is valid pointer from Box::into_raw.
        drop(unsafe { Box::from_raw(handle) });
    });
}

/// Free a `Box`-allocated FFI handle with pre-free teardown (V1 — void return).
///
/// Sequence: null check → panic catch → `assert_not_freed` → `mark_freed` → teardown → `Box::from_raw` + drop.
///
/// Panics (caught by unwind guard) on double-free via `assert_not_freed`.
///
/// # Safety
///
/// - `handle` must be null or a valid pointer from `Box::into_raw`.
/// - `handle` must not have been freed previously (panics on double-free).
/// - The `teardown` closure must not deallocate `handle`.
pub unsafe fn box_handle_free_v1_with_teardown<H, F>(
    name: &'static str,
    handle: *mut H,
    tracker: FfiTracker,
    teardown: F,
) where
    F: FnOnce(*mut H),
{
    crate::aterm_ffi_catch_unwind!((), { /* panic caught at FFI boundary */ }, {
        if handle.is_null() {
            return;
        }
        if tracker.is_freed(handle.cast::<c_void>()) {
            tracker.assert_not_freed(handle.cast::<c_void>());
        }
        if !tracker.is_allocated(handle.cast::<c_void>()) {
            aterm_log::error!("{}: rejecting free of untracked handle {:p}", name, handle);
            return;
        }
        tracker.assert_not_freed(handle.cast::<c_void>());
        tracker.mark_freed(handle.cast::<c_void>());
        teardown(handle);
        // SAFETY: Caller guarantees handle is valid pointer from Box::into_raw.
        drop(unsafe { Box::from_raw(handle) });
    });
}

/// Free a `Box`-allocated FFI handle with caller-specified null error (V2).
///
/// Like [`box_handle_free_v2`] but the caller provides the null-handle error
/// value directly. This supports error types where `null_handle()` returns a
/// generic sentinel but individual functions need handle-specific null variants
/// (e.g., `AtermGpuError::ErrNullPathBuilder` vs `ErrNullCanvas`).
///
/// # Safety
///
/// - `handle` must be null or a valid pointer from `Box::into_raw`.
/// - `handle` must not have been freed previously (detected, not UB).
pub unsafe fn box_handle_free_v2_with_null<H, E: FfiErrorCode>(
    name: &'static str,
    handle: *mut H,
    tracker: FfiTracker,
    null_err: E,
) -> E {
    crate::aterm_ffi_catch_unwind!(E::internal(), { /* panic caught at FFI boundary */ }, {
        if handle.is_null() {
            return null_err;
        }
        if tracker.is_freed(handle.cast::<c_void>()) {
            return E::double_free();
        }
        if !tracker.is_allocated(handle.cast::<c_void>()) {
            aterm_log::error!("{}: rejecting free of untracked handle {:p}", name, handle);
            return E::internal();
        }
        tracker.mark_freed(handle.cast::<c_void>());
        // SAFETY: Caller guarantees handle is valid pointer from Box::into_raw.
        drop(unsafe { Box::from_raw(handle) });
        E::ok()
    })
}

/// Free a `Box`-allocated FFI handle through a pointer-to-pointer with null-after-free (V2).
///
/// Takes `*mut *mut H` (pointer-to-pointer), reads the inner handle, frees it,
/// then writes null back to the caller's variable. This prevents use-after-free
/// because subsequent dereferences see null and get an error code, not UB.
///
/// Sequence: outer-null check → read inner → inner-null check → `mark_freed` → `Box::from_raw` → null-after-free.
///
/// # Safety
///
/// - `handle_ptr` must be null or a valid pointer to a `*mut H`.
/// - The inner `*handle_ptr` must be null or a valid pointer from `Box::into_raw`.
/// - The inner pointer must not have been freed previously (detected, not UB).
pub unsafe fn box_handle_free_v2_nulling<H, E: FfiErrorCode>(
    name: &'static str,
    handle_ptr: *mut *mut H,
    tracker: FfiTracker,
    null_err: E,
) -> E {
    crate::aterm_ffi_catch_unwind!(E::internal(), { /* panic caught at FFI boundary */ }, {
        if handle_ptr.is_null() {
            return null_err;
        }
        // SAFETY: Caller guarantees handle_ptr is valid for read/write.
        let inner = unsafe { *handle_ptr };
        if inner.is_null() {
            return null_err;
        }
        if tracker.is_freed(inner.cast::<c_void>()) {
            return E::double_free();
        }
        if !tracker.is_allocated(inner.cast::<c_void>()) {
            aterm_log::error!("{}: rejecting free of untracked handle {:p}", name, inner);
            return E::internal();
        }
        tracker.mark_freed(inner.cast::<c_void>());
        // SAFETY: Caller guarantees inner is valid pointer from Box::into_raw.
        drop(unsafe { Box::from_raw(inner) });
        // Null out the caller's handle to prevent use-after-free.
        // SAFETY: handle_ptr validated non-null above; caller guarantees writable memory.
        unsafe { *handle_ptr = std::ptr::null_mut() };
        E::ok()
    })
}

/// Free a `CString`-allocated FFI string (V1 — void return).
///
/// Sequence: null check → panic catch → `assert_not_freed` → `mark_freed` → `CString::from_raw` + drop.
///
/// Panics (caught by unwind guard) on double-free via `assert_not_freed`.
///
/// # Safety
///
/// - `ptr` must be null or a valid pointer from `CString::into_raw`.
/// - `ptr` must not have been freed previously (panics on double-free).
pub unsafe fn cstring_handle_free_v1(
    _name: &'static str,
    ptr: *mut std::os::raw::c_char,
    tracker: FfiTracker,
) {
    crate::aterm_ffi_catch_unwind!((), { /* panic caught at FFI boundary */ }, {
        if ptr.is_null() {
            return;
        }
        tracker.assert_not_freed(ptr.cast::<c_void>());
        tracker.mark_freed(ptr.cast::<c_void>());
        // SAFETY: Caller guarantees ptr is valid pointer from CString::into_raw.
        drop(unsafe { CString::from_raw(ptr) });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verification::ffi_free_tracker;

    #[test]
    fn box_free_v2_null_returns_null_handle() {
        let result: crate::AtermTerminalError = unsafe {
            box_handle_free_v2::<u64, _>("test", std::ptr::null_mut(), FfiTracker::General)
        };
        assert_eq!(result, crate::AtermTerminalError::ErrNullTerminal);
    }

    #[test]
    fn box_free_v2_valid_pointer_returns_ok() {
        let val = Box::into_raw(Box::new(42u64));
        ffi_free_tracker::mark_allocated(val.cast());

        let result: crate::AtermTerminalError =
            unsafe { box_handle_free_v2("test", val, FfiTracker::General) };
        assert_eq!(result, crate::AtermTerminalError::Ok);
    }

    #[test]
    fn box_free_v2_double_free_returns_error() {
        let val = Box::into_raw(Box::new(42u64));
        ffi_free_tracker::mark_allocated(val.cast());

        let first: crate::AtermTerminalError =
            unsafe { box_handle_free_v2("test", val, FfiTracker::General) };
        assert_eq!(first, crate::AtermTerminalError::Ok);

        // Second free on same pointer — should detect double-free.
        let second: crate::AtermTerminalError =
            unsafe { box_handle_free_v2("test", val, FfiTracker::General) };
        assert_eq!(second, crate::AtermTerminalError::ErrDoubleFree);
    }

    #[test]
    fn box_free_v2_with_teardown_runs_closure() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let val = Box::into_raw(Box::new(42u64));
        ffi_free_tracker::mark_allocated(val.cast());
        let teardown_ran = AtomicBool::new(false);

        let result: crate::AtermTerminalError = unsafe {
            box_handle_free_v2_with_teardown("test", val, FfiTracker::General, |_ptr| {
                teardown_ran.store(true, Ordering::Relaxed);
            })
        };
        assert_eq!(result, crate::AtermTerminalError::Ok);
        assert!(teardown_ran.load(Ordering::Relaxed));
    }

    #[test]
    fn box_free_v1_null_is_noop() {
        unsafe { box_handle_free_v1::<u64>("test", std::ptr::null_mut(), FfiTracker::General) };
        // No panic — passes
    }

    #[test]
    fn box_free_v1_valid_pointer_succeeds() {
        let val = Box::into_raw(Box::new(99u64));
        ffi_free_tracker::mark_allocated(val.cast());

        unsafe { box_handle_free_v1("test", val, FfiTracker::General) };
        // Pointer is now freed
        assert!(ffi_free_tracker::is_freed(val.cast()));
    }

    #[test]
    fn cstring_free_v1_null_is_noop() {
        unsafe { cstring_handle_free_v1("test", std::ptr::null_mut(), FfiTracker::General) };
        // No panic — passes
    }

    #[test]
    fn cstring_free_v1_valid_pointer_succeeds() {
        let s = CString::new("hello").expect("valid CString");
        let ptr = s.into_raw();
        ffi_free_tracker::mark_allocated(ptr.cast());

        unsafe { cstring_handle_free_v1("test", ptr, FfiTracker::General) };
        assert!(ffi_free_tracker::is_freed(ptr.cast()));
    }

    // ── V1 double-free detection ───────────────────────────────────────

    #[test]
    fn box_free_v1_double_free_is_caught_by_unwind() {
        let val = Box::into_raw(Box::new(42u64));
        ffi_free_tracker::mark_allocated(val.cast());

        unsafe { box_handle_free_v1("test", val, FfiTracker::General) };
        // Second free: assert_not_freed panics, caught by catch_unwind — no crash.
        unsafe { box_handle_free_v1("test", val, FfiTracker::General) };
    }

    #[test]
    fn cstring_free_v1_double_free_is_caught_by_unwind() {
        let s = CString::new("world").expect("valid CString");
        let ptr = s.into_raw();
        ffi_free_tracker::mark_allocated(ptr.cast());

        unsafe { cstring_handle_free_v1("test", ptr, FfiTracker::General) };
        // Second free: assert_not_freed panics, caught by catch_unwind — no crash.
        unsafe { cstring_handle_free_v1("test", ptr, FfiTracker::General) };
    }

    // ── V1 with teardown ───────────────────────────────────────────────

    #[test]
    fn box_free_v1_with_teardown_runs_closure() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let val = Box::into_raw(Box::new(77u64));
        ffi_free_tracker::mark_allocated(val.cast());
        let teardown_ran = AtomicBool::new(false);

        unsafe {
            box_handle_free_v1_with_teardown("test", val, FfiTracker::General, |_ptr| {
                teardown_ran.store(true, Ordering::Relaxed);
            });
        }
        assert!(teardown_ran.load(Ordering::Relaxed));
        assert!(ffi_free_tracker::is_freed(val.cast()));
    }

    #[test]
    fn box_free_v1_with_teardown_null_skips_closure() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let teardown_ran = AtomicBool::new(false);
        unsafe {
            box_handle_free_v1_with_teardown::<u64, _>(
                "test",
                std::ptr::null_mut(),
                FfiTracker::General,
                |_ptr| {
                    teardown_ran.store(true, Ordering::Relaxed);
                },
            );
        }
        assert!(!teardown_ran.load(Ordering::Relaxed));
    }

    #[test]
    fn box_free_v1_with_teardown_double_free_skips_closure() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let val = Box::into_raw(Box::new(88u64));
        ffi_free_tracker::mark_allocated(val.cast());
        let call_count = AtomicU32::new(0);

        // First free: teardown runs.
        unsafe {
            box_handle_free_v1_with_teardown("test", val, FfiTracker::General, |_ptr| {
                call_count.fetch_add(1, Ordering::Relaxed);
            });
        }
        assert_eq!(call_count.load(Ordering::Relaxed), 1);

        // Second free: assert_not_freed panics before teardown — caught by unwind.
        unsafe {
            box_handle_free_v1_with_teardown("test", val, FfiTracker::General, |_ptr| {
                call_count.fetch_add(1, Ordering::Relaxed);
            });
        }
        // Teardown did NOT run on the second call.
        assert_eq!(call_count.load(Ordering::Relaxed), 1);
    }

    // ── V2 with teardown edge cases ────────────────────────────────────

    #[test]
    fn box_free_v2_with_teardown_null_skips_closure() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let teardown_ran = AtomicBool::new(false);
        let result: crate::AtermTerminalError = unsafe {
            box_handle_free_v2_with_teardown::<u64, _, _>(
                "test",
                std::ptr::null_mut(),
                FfiTracker::General,
                |_ptr| {
                    teardown_ran.store(true, Ordering::Relaxed);
                },
            )
        };
        assert_eq!(result, crate::AtermTerminalError::ErrNullTerminal);
        assert!(!teardown_ran.load(Ordering::Relaxed));
    }

    #[test]
    fn box_free_v2_with_teardown_double_free_skips_closure() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let val = Box::into_raw(Box::new(55u64));
        ffi_free_tracker::mark_allocated(val.cast());
        let call_count = AtomicU32::new(0);

        // First free: teardown runs.
        let first: crate::AtermTerminalError = unsafe {
            box_handle_free_v2_with_teardown("test", val, FfiTracker::General, |_ptr| {
                call_count.fetch_add(1, Ordering::Relaxed);
            })
        };
        assert_eq!(first, crate::AtermTerminalError::Ok);
        assert_eq!(call_count.load(Ordering::Relaxed), 1);

        // Second free: returns ErrDoubleFree, teardown NOT called.
        let second: crate::AtermTerminalError = unsafe {
            box_handle_free_v2_with_teardown("test", val, FfiTracker::General, |_ptr| {
                call_count.fetch_add(1, Ordering::Relaxed);
            })
        };
        assert_eq!(second, crate::AtermTerminalError::ErrDoubleFree);
        assert_eq!(call_count.load(Ordering::Relaxed), 1);
    }

    // ── FfiTracker::Terminal path ──────────────────────────────────────

    #[test]
    fn box_free_v2_terminal_tracker_works() {
        use crate::verification::terminal_handle_tracker;

        let val = Box::into_raw(Box::new(123u64));
        terminal_handle_tracker::mark_allocated(val.cast());

        let result: crate::AtermTerminalError =
            unsafe { box_handle_free_v2("test", val, FfiTracker::Terminal) };
        assert_eq!(result, crate::AtermTerminalError::Ok);
        assert!(terminal_handle_tracker::is_freed(val.cast()));
    }

    #[test]
    fn box_free_v2_untracked_pointer_returns_internal_without_freeing() {
        let anchor = 123u64;
        let val = std::ptr::addr_of!(anchor).cast_mut();

        let result: crate::AtermTerminalError =
            unsafe { box_handle_free_v2("test", val, FfiTracker::General) };
        assert_eq!(result, crate::AtermTerminalError::ErrInternal);
    }
}
