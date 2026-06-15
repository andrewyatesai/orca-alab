// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Shared panic-catching helpers for FFI boundary macros.

/// Catch panics at an FFI boundary and return a default value on panic.
///
/// This macro centralizes the `catch_unwind` implementation so domain crates can
/// customize logging/prefix behavior while sharing one safety primitive.
///
/// # Arguments
///
/// - `$default`: value returned when a panic is caught.
/// - `$on_panic`: expression executed when a panic is caught (for logging).
/// - `$body`: FFI function body to execute.
#[macro_export]
macro_rules! aterm_ffi_catch_unwind {
    ($default:expr_2021, $on_panic:expr_2021, $body:expr_2021) => {{
        #[cfg(kani)]
        {
            // Kani cannot model `catch_unwind` (kani#267); verify inner logic directly.
            $body
        }
        #[cfg(not(kani))]
        {
            match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| $body)) {
                Ok(result) => result,
                Err(_panic) => {
                    // Extract panic message for diagnostics (#5892, F11-2 #7941).
                    //
                    // Never silently mask an FFI panic: log to stderr *and* to
                    // the structured log sink so observability pipelines see
                    // it even when the `ffi-logging` feature is off.
                    let _msg = _panic
                        .downcast_ref::<&str>()
                        .copied()
                        .or_else(|| _panic.downcast_ref::<String>().map(|s| s.as_str()));
                    let _msg_str = _msg.unwrap_or("<non-string panic payload>");
                    eprintln!("[aterm-ffi] panic caught: {_msg_str}");
                    $crate::aterm_log::error!(
                        "[aterm-ffi] FFI call panicked — returning error code: {}",
                        _msg_str
                    );
                    $on_panic;
                    $default
                }
            }
        }
    }};
}

/// Catch panics at an FFI boundary with crate-specific logging prefix support.
///
/// This macro wraps `aterm_ffi_catch_unwind!` and standardizes panic logging
/// format while letting each FFI domain specify its own log prefix.
///
/// # Arguments
///
/// - `$log_prefix`: logging prefix (for example `"[aterm-editor-ffi]"`).
/// - `$default`: value returned when a panic is caught.
/// - `$fn_name`: function name used in panic logs.
/// - `$body`: FFI function body to execute.
#[macro_export]
macro_rules! aterm_ffi_catch_panic {
    ($log_prefix:literal, $default:expr_2021, $fn_name:literal, $body:expr_2021) => {
        $crate::aterm_ffi_catch_unwind!(
            $default,
            {
                // F11-2 (#7941): log the prefix+fn_name even without
                // the `ffi-logging` feature so panic attribution is
                // never silently dropped.
                $crate::aterm_log::error!("{} {}: panic caught", $log_prefix, $fn_name);
            },
            $body
        )
    };
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn shared_macro_returns_body_value_on_success() {
        let ran = AtomicBool::new(false);
        let value: i32 = aterm_ffi_catch_unwind!(-1, { ran.store(true, Ordering::Relaxed) }, { 7 });
        assert_eq!(value, 7);
        assert!(
            !ran.load(Ordering::Relaxed),
            "on_panic should not run on success"
        );
    }

    #[test]
    fn shared_macro_returns_default_on_panic() {
        let ran = AtomicBool::new(false);
        let value: i32 = aterm_ffi_catch_unwind!(-1, { ran.store(true, Ordering::Relaxed) }, {
            panic!("boom");
        });
        assert_eq!(value, -1);
        assert!(ran.load(Ordering::Relaxed), "on_panic should run on panic");
    }

    #[test]
    fn panic_macro_uses_default_on_panic() {
        let value: i32 = aterm_ffi_catch_panic!("[aterm-test-ffi]", -1, "test_fn", {
            panic!("boom");
        });
        assert_eq!(value, -1);
    }

    #[test]
    fn panic_macro_returns_body_value() {
        let value: i32 = aterm_ffi_catch_panic!("[aterm-test-ffi]", -1, "test_fn", { 11 });
        assert_eq!(value, 11);
    }
}
