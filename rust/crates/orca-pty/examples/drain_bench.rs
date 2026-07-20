//! Drain-ceiling benchmark for the daemon's PTY pump (`orca-daemon::pump_output`).
//!
//! Measures how fast orca can drain a `cat <corpus>` flood off the PTY master —
//! the embedded-terminal analogue of aterm-gui's on-glass cat-flood. Two drain
//! shapes, drain-only (no engine) to isolate the read side:
//!   * `blocking` — the current pump: one blocking `read()` per loop, 64 KiB buf.
//!     macOS caps each master read at ~1 KiB, so this locksteps with the writer.
//!   * `gather`   — the aterm-pty shape: O_NONBLOCK, drain to EAGAIN into a 64 KiB
//!     batch with a bounded spin/poll bridge over refill gaps, THEN hand off.
//!
//! Run: cargo run --release --example drain_bench -- /tmp/atbench/flood_500.vt
#[cfg(unix)]
fn main() {
    use orca_pty::{PtyCommand, PtySession, PtySize};
    use std::time::Instant;

    let corpus = std::env::args().nth(1).unwrap_or_else(|| "/tmp/atbench/flood_500.vt".into());
    let bytes = std::fs::metadata(&corpus).expect("corpus stat").len();
    let mb = bytes as f64 / 1e6;
    let mode = std::env::args().nth(2).unwrap_or_else(|| "both".into());

    // One trial: spawn `cat corpus`, drain the master to EOF, return MB/s.
    let run_blocking = || -> f64 {
        let cmd = PtyCommand {
            program: "cat".into(),
            args: vec![corpus.clone()],
            ..Default::default()
        };
        let session = PtySession::spawn(&cmd, PtySize { rows: 40, cols: 120 }).expect("spawn");
        let mut reader = session.try_clone_reader().expect("reader");
        let mut buf = [0u8; 65536];
        let t0 = Instant::now();
        let mut total = 0u64;
        loop {
            match std::io::Read::read(&mut reader, &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => total += n as u64,
            }
        }
        let dt = t0.elapsed().as_secs_f64();
        assert!(total >= bytes, "drained {total} < corpus {bytes}");
        mb / dt
    };

    // The production gather (orca_pty::gather_drain over an owned nonblocking dup)
    // — the exact code the daemon pump uses.
    let run_gather = || -> f64 {
        let cmd = PtyCommand {
            program: "cat".into(),
            args: vec![corpus.clone()],
            ..Default::default()
        };
        let session = PtySession::spawn(&cmd, PtySize { rows: 40, cols: 120 }).expect("spawn");
        let read_fd = session.clone_read_fd().expect("clone_read_fd");
        let fd = read_fd.as_raw_fd();
        let mut batch = vec![0u8; 65536];
        let t0 = Instant::now();
        let mut total = 0u64;
        loop {
            let (filled, eof) = orca_pty::gather_drain(fd, &mut batch);
            total += filled as u64;
            if eof {
                break;
            }
        }
        let dt = t0.elapsed().as_secs_f64();
        assert!(total >= bytes, "drained {total} < corpus {bytes}");
        mb / dt
    };

    let trials = 3;
    let median = |mut v: Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    println!("corpus {corpus} = {mb:.1} MB, {trials} trials each");
    if mode == "blocking" || mode == "both" {
        let r: Vec<f64> = (0..trials).map(|_| run_blocking()).collect();
        println!("blocking (current pump): median {:.1} MB/s  {:?}", median(r.clone()), r);
    }
    if mode == "gather" || mode == "both" {
        let r: Vec<f64> = (0..trials).map(|_| run_gather()).collect();
        println!("gather   (aterm shape):  median {:.1} MB/s  {:?}", median(r.clone()), r);
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("drain_bench is unix-only");
}
