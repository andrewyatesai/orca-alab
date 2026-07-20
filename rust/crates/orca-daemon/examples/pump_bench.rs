//! End-to-end pump-throughput bench: how fast orca's daemon drains a `cat` flood
//! AND feeds the real headless aterm engine (`terminal.process`) — the honest
//! embedded-terminal number, the socket route (already a v1020 binary plane)
//! aside. Two shapes matching `orca-daemon::pump_output`:
//!   * `blocking` — the old pump: one blocking read (≈1 KiB on macOS) → process,
//!     per read. Locksteps the engine feed with the writer.
//!   * `gather`   — the graft: O_NONBLOCK gather to a 64 KiB batch → ONE process
//!     per batch, so the per-feed cost amortizes over the whole batch.
//! ASCII corpus ⇒ the barrier-less raw feed (no UTF-8 decode) is exact.
//!
//! Run: cargo run --release --example pump_bench -- /tmp/atbench/flood_500.vt
#[cfg(unix)]
fn main() {
    use orca_pty::{gather_drain, PtyCommand, PtySession, PtySize};
    use orca_terminal::{HeadlessTerminal, DEFAULT_SCROLLBACK};
    use std::time::Instant;

    let corpus = std::env::args().nth(1).unwrap_or_else(|| "/tmp/atbench/flood_500.vt".into());
    let bytes = std::fs::metadata(&corpus).expect("corpus stat").len();
    let mb = bytes as f64 / 1e6;

    let spawn = || {
        let cmd = PtyCommand {
            program: "cat".into(),
            args: vec![corpus.clone()],
            ..Default::default()
        };
        PtySession::spawn(&cmd, PtySize { rows: 40, cols: 120 }).expect("spawn")
    };

    // blocking: read → feed engine, per read (the old pump).
    let run_blocking = || -> f64 {
        let session = spawn();
        let mut reader = session.try_clone_reader().expect("reader");
        let mut term = HeadlessTerminal::with_scrollback(40, 120, DEFAULT_SCROLLBACK);
        let mut buf = [0u8; 65536];
        let t0 = Instant::now();
        let mut total = 0u64;
        loop {
            match std::io::Read::read(&mut reader, &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    term.process(&buf[..n]);
                    total += n as u64;
                }
            }
        }
        let dt = t0.elapsed().as_secs_f64();
        assert!(total >= bytes, "drained {total} < corpus {bytes}");
        mb / dt
    };

    // gather: drain to a 64 KiB batch → feed engine ONCE per batch (the graft).
    let run_gather = || -> f64 {
        let session = spawn();
        let read_fd = session.clone_read_fd().expect("clone_read_fd");
        let fd = read_fd.as_raw_fd();
        let mut term = HeadlessTerminal::with_scrollback(40, 120, DEFAULT_SCROLLBACK);
        let mut buf = [0u8; 65536];
        let t0 = Instant::now();
        let mut total = 0u64;
        loop {
            let (filled, eof) = gather_drain(fd, &mut buf);
            if filled > 0 {
                term.process(&buf[..filled]);
                total += filled as u64;
            }
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
    println!("corpus {corpus} = {mb:.1} MB, {trials} trials each (drain + real engine.process)");
    let b: Vec<f64> = (0..trials).map(|_| run_blocking()).collect();
    println!("blocking (old pump): median {:.1} MB/s  {:?}", median(b.clone()), b);
    let g: Vec<f64> = (0..trials).map(|_| run_gather()).collect();
    println!("gather   (grafted):  median {:.1} MB/s  {:?}", median(g.clone()), g);
}

#[cfg(not(unix))]
fn main() {
    eprintln!("pump_bench is unix-only");
}
