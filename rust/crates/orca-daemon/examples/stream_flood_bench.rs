//! End-to-end daemon flood throughput: real `serve()` on a Unix socket, a real
//! control+stream client pair, and a `cat <corpus>` session — so the measured
//! path is PTY read → decode → engine feed → route_output → stream writer
//! (coalescing) → socket → client read. This is the harness for the P2
//! writer-side coalescing numbers (docs/rust-migration/daemon-pty-drain-
//! investigation.md); the pump_bench example measures only the read+engine leg.
//!
//! Run on a QUIET machine, interleaved before/after builds (ABBA):
//!   cargo run --release -p orca-daemon --example stream_flood_bench -- \
//!     <corpus-path> [--mb 200] [--binary]
//! A missing corpus file is generated deterministically (~--mb MB of SGR-
//! colored text lines). Reported MB/s = corpus bytes / (createOrAttach → exit
//! event); wire MB/s counts actual socket bytes (OPOST inflates \n → \r\n).
#[cfg(unix)]
fn main() {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Instant;

    let mut args = std::env::args().skip(1);
    let corpus = args.next().expect("usage: stream_flood_bench <corpus-path> [--mb N] [--binary]");
    let mut corpus_mb: usize = 200;
    let mut binary = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--mb" => corpus_mb = args.next().and_then(|v| v.parse().ok()).expect("--mb N"),
            "--binary" => binary = true,
            other => panic!("unknown arg {other}"),
        }
    }
    if !std::path::Path::new(&corpus).exists() {
        generate_corpus(&corpus, corpus_mb);
    }
    let bytes = std::fs::metadata(&corpus).expect("corpus stat").len();
    let mb = bytes as f64 / 1e6;

    // In-process daemon on a throwaway socket: same serve()/connection/registry
    // code as production, no token gate (parity-harness mode).
    let sock_dir = std::env::temp_dir().join(format!("orca-flood-{}", std::process::id()));
    std::fs::create_dir_all(&sock_dir).expect("sock dir");
    let sock = sock_dir.join("daemon.sock").to_string_lossy().into_owned();
    {
        let sock = sock.clone();
        std::thread::spawn(move || {
            let _ = orca_daemon::serve(&sock, None);
        });
    }
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    while !std::path::Path::new(&sock).exists() && Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let version = orca_daemon::protocol::PROTOCOL_VERSION;
    let hello = |role: &str| {
        let fmt = if binary && role == "stream" { r#","streamFormat":"binary""# } else { "" };
        format!(r#"{{"type":"hello","version":{version},"token":"","clientId":"flood","role":"{role}"{fmt}}}"#)
    };
    let mut control = UnixStream::connect(&sock).expect("control connect");
    control.write_all(format!("{}\n", hello("control")).as_bytes()).expect("control hello");
    let mut control_reader = BufReader::new(control.try_clone().expect("clone"));
    let mut line = String::new();
    control_reader.read_line(&mut line).expect("control hello reply");
    assert!(line.contains(r#""ok":true"#), "control hello rejected: {line}");

    let mut stream = UnixStream::connect(&sock).expect("stream connect");
    stream.write_all(format!("{}\n", hello("stream")).as_bytes()).expect("stream hello");
    let mut reply = Vec::new();
    let mut one = [0u8; 1];
    while stream.read_exact(&mut one).is_ok() {
        if one[0] == b'\n' {
            break;
        }
        reply.push(one[0]);
    }
    assert!(String::from_utf8_lossy(&reply).contains(r#""ok":true"#), "stream hello rejected");

    // /bin/sh -c cat: the child argv IS the flood (no login-shell rc noise).
    let req = format!(
        r#"{{"id":"c1","type":"createOrAttach","payload":{{"sessionId":"flood","cols":120,"rows":40,"shellOverride":"/bin/sh","shellArgs":["-c","cat {corpus}"]}}}}"#
    );
    let t0 = Instant::now();
    control.write_all(format!("{req}\n").as_bytes()).expect("createOrAttach");

    // Drain the stream socket raw until the session's exit event; a 64-byte tail
    // overlap catches the marker when it straddles a read boundary.
    let mut buf = vec![0u8; 256 * 1024];
    let mut tail: Vec<u8> = Vec::new();
    let mut wire_total = 0u64;
    let needle = br#""event":"exit""#;
    loop {
        let n = stream.read(&mut buf).expect("stream read");
        assert!(n > 0, "stream closed before exit event");
        wire_total += n as u64;
        let mut scan = std::mem::take(&mut tail);
        scan.extend_from_slice(&buf[..n]);
        if scan.windows(needle.len()).any(|w| w == needle) {
            break;
        }
        let keep = scan.len().min(64);
        tail = scan[scan.len() - keep..].to_vec();
    }
    let secs = t0.elapsed().as_secs_f64();
    println!(
        "corpus {:.1} MB  mode {}  elapsed {:.3}s  corpus {:.1} MB/s  wire {:.1} MB/s",
        mb,
        if binary { "binary" } else { "ndjson" },
        secs,
        mb / secs,
        wire_total as f64 / 1e6 / secs,
    );
    // Close our sockets and give the daemon's per-client threads a beat to tear
    // down (lets any daemon-side diagnostics flush) before killing serve().
    drop(stream);
    drop(control);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&sock_dir);
    std::process::exit(0); // serve() thread never returns on its own
}

/// Deterministic flood corpus: `mb` MB of 100-char SGR-colored ASCII lines —
/// the balanced cat-flood shape from the drain investigation, reproducible
/// without the original scratchpad corpus files.
#[cfg(unix)]
fn generate_corpus(path: &str, mb: usize) {
    use std::io::Write;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).expect("corpus create"));
    let mut written = 0usize;
    let mut i = 0u64;
    while written < mb * 1_000_000 {
        let line = format!(
            "\x1b[3{}mINFO\x1b[0m step {:010} lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod\n",
            i % 8,
            i
        );
        f.write_all(line.as_bytes()).expect("corpus write");
        written += line.len();
        i += 1;
    }
    f.flush().expect("corpus flush");
}

#[cfg(not(unix))]
fn main() {
    eprintln!("stream_flood_bench is unix-only (UnixStream client); the daemon itself runs on Windows named pipes.");
}
