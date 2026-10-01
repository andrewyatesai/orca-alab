//! The stream-throughput bench refuses a zero chunk size as a usage error
//! (`chunks(0)` would otherwise panic mid-run).

#![cfg(unix)]

use std::process::Command;

#[test]
fn zero_chunk_bytes_is_refused_before_any_io() {
    // Paths that do not exist: the refusal must come before the corpus read or
    // the socket bind.
    let out = Command::new(env!("CARGO_BIN_EXE_stream-throughput-bench"))
        .args(["/nonexistent/sock", "/nonexistent/corpus", "binary", "0"])
        .output()
        .expect("run the bench binary");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("chunk_bytes must be a positive byte count"),
        "stderr: {stderr}"
    );
}
