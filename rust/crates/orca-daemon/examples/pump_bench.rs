//! Evidence for docs/rust-migration/daemon-pty-drain-investigation.md: the
//! O_NONBLOCK gather-drain that wins on aterm-gui's on-glass cat-flood REGRESSES
//! orca's single-threaded daemon pump, because the blocking loop already
//! pipelines the engine feed with `cat` through the kernel PTY buffer and
//! batching serializes drain-then-process. Both shapes feed the REAL headless
//! aterm engine (`terminal.process`); ASCII corpus ⇒ no UTF-8/OPOST rewrite.
//!
//! Run on a QUIET machine (loadavg < ~3 — under load the gather's busy-reads
//! steal cores from `cat` and the numbers invert unreliably):
//!   cargo run --release -p orca-daemon --example pump_bench -- /tmp/atbench/flood_500.vt
#[cfg(unix)]
fn main() {
    use nix::errno::Errno;
    use nix::fcntl::{fcntl, FcntlArg, OFlag};
    use nix::poll::{PollFd, PollFlags};
    use orca_pty::{PtyCommand, PtySession, PtySize};
    use orca_terminal::{HeadlessTerminal, DEFAULT_SCROLLBACK};
    use std::time::{Duration, Instant};

    let corpus = std::env::args().nth(1).unwrap_or_else(|| "/tmp/atbench/flood_500.vt".into());
    let bytes = std::fs::metadata(&corpus).expect("corpus stat").len();
    let mb = bytes as f64 / 1e6;

    let spawn = || {
        PtySession::spawn(
            &PtyCommand { program: "cat".into(), args: vec![corpus.clone()], ..Default::default() },
            PtySize { rows: 40, cols: 120 },
        )
        .expect("spawn")
    };

    // The current pump: one blocking read (≈1 KiB on macOS) → feed engine, per read.
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
        assert!(total >= bytes);
        mb / t0.elapsed().as_secs_f64()
    };

    // The rejected graft: O_NONBLOCK gather to a 64 KiB batch → feed engine once.
    let run_gather = || -> f64 {
        let session = spawn();
        let dup = nix::unistd::dup(session.master_raw_fd().expect("raw fd")).expect("dup");
        let flags = OFlag::from_bits_truncate(fcntl(dup, FcntlArg::F_GETFL).expect("getfl"));
        fcntl(dup, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK)).expect("nonblock");
        let mut term = HeadlessTerminal::with_scrollback(40, 120, DEFAULT_SCROLLBACK);
        let mut buf = [0u8; 65536];
        let t0 = Instant::now();
        let (mut total, mut eof) = (0u64, false);
        while !eof {
            let (mut filled, mut spins) = (0usize, 0u32);
            let start = Instant::now();
            loop {
                if filled == buf.len() {
                    break;
                }
                match nix::unistd::read(dup, &mut buf[filled..]) {
                    Ok(0) => {
                        eof = true;
                        break;
                    }
                    Ok(n) => {
                        filled += n;
                        spins = 0;
                    }
                    Err(Errno::EINTR) => continue,
                    Err(Errno::EAGAIN) => {
                        if filled == 0 {
                            let mut fds = [PollFd::new(dup, PollFlags::POLLIN)];
                            let _ = nix::poll::poll(&mut fds, -1);
                            continue;
                        }
                        if filled < 1024 || start.elapsed() >= Duration::from_millis(3) {
                            break;
                        }
                        if spins < 16 {
                            spins += 1;
                            continue;
                        }
                        let mut fds = [PollFd::new(dup, PollFlags::POLLIN)];
                        match nix::poll::poll(&mut fds, 1) {
                            Ok(0) | Err(_) => break,
                            Ok(_) => spins = 0,
                        }
                    }
                    Err(_) => {
                        eof = true;
                        break;
                    }
                }
            }
            if filled > 0 {
                term.process(&buf[..filled]);
                total += filled as u64;
            }
        }
        let _ = nix::unistd::close(dup);
        assert!(total >= bytes);
        mb / t0.elapsed().as_secs_f64()
    };

    let median = |mut v: Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    println!("corpus {corpus} = {mb:.1} MB, 3 trials each (drain + real engine.process)");
    let b: Vec<f64> = (0..3).map(|_| run_blocking()).collect();
    println!("blocking (current pump): median {:.1} MB/s  {:?}", median(b.clone()), b);
    let g: Vec<f64> = (0..3).map(|_| run_gather()).collect();
    println!("gather   (rejected):     median {:.1} MB/s  {:?}", median(g.clone()), g);
}

#[cfg(not(unix))]
fn main() {
    eprintln!("pump_bench is unix-only");
}
