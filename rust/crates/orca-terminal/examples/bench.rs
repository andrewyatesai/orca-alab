//! Head-to-head terminal-parsing benchmark: the Rust `HeadlessTerminal` engine
//! against the `@xterm/headless` engine Orca currently ships (see the Node
//! counterpart at `tools/terminal-bench/xterm-bench.mjs`).
//!
//! Both engines consume the *same* corpus file (generated here, deterministically,
//! so there is a single source of truth) in the same chunk size, then dump the
//! final visible grid for a parity check. Usage:
//!   cargo run -q --release --example bench -- gen   <corpus> <megabytes>
//!   cargo run -q --release --example bench -- run   <corpus> <out.json>

use std::env;
use std::fs;
use std::io::Write;
use std::time::Instant;

use orca_terminal::HeadlessTerminal;

const ROWS: usize = 40;
const COLS: usize = 120;
const SCROLLBACK: usize = 5000;
const CHUNK: usize = 4096; // mirrors a typical PTY read size

fn main() {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("gen") => {
            let path = &args[2];
            let mb: usize = args[3].parse().expect("megabytes");
            let corpus = generate_corpus(mb * 1024 * 1024);
            fs::write(path, &corpus).expect("write corpus");
            println!("wrote {} ({} bytes)", path, corpus.len());
        }
        Some("run") => {
            let path = &args[2];
            let out = args.get(3).cloned();
            let iters: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(1);
            run(path, out, iters);
        }
        _ => {
            eprintln!("usage: bench gen <corpus> <mb> | bench run <corpus> [out.json]");
            std::process::exit(2);
        }
    }
}

fn run(path: &str, out: Option<String>, iters: usize) {
    let corpus = fs::read(path).expect("read corpus");

    // For profiling, loop the whole parse `iters` times (fresh terminal each)
    // so the process runs long enough to sample. Timing covers all iterations;
    // throughput is normalized to one pass.
    // Measurement toggle for the scrollback-text-only optimization:
    // with_scrollback enables it; ATERM_SB_TEXT_ONLY=0 turns it back off so both
    // can be timed interleaved in one binary.
    let text_only = std::env::var("ATERM_SB_TEXT_ONLY").map(|v| v != "0").unwrap_or(true);
    let mut term = HeadlessTerminal::with_scrollback(ROWS, COLS, SCROLLBACK);
    aterm_grid::set_scrollback_text_only(text_only);
    let start = Instant::now();
    for _ in 0..iters {
        term = HeadlessTerminal::with_scrollback(ROWS, COLS, SCROLLBACK);
        aterm_grid::set_scrollback_text_only(text_only);
        for chunk in corpus.chunks(CHUNK) {
            term.process(chunk);
        }
    }
    let elapsed = start.elapsed() / iters as u32;

    let mb = corpus.len() as f64 / (1024.0 * 1024.0);
    let secs = elapsed.as_secs_f64();
    let throughput = mb / secs;

    // Final visible grid — the parity fingerprint both engines must agree on.
    let visible = term.snapshot().join("\n");

    eprintln!(
        "rust  : {:.2} MB in {:.1} ms = {:.1} MB/s  (scrollback {} lines)",
        mb,
        secs * 1000.0,
        throughput,
        term.scrollback_len()
    );

    if let Some(out) = out {
        let json = format!(
            "{{\"engine\":\"rust-orca-terminal\",\"bytes\":{},\"ms\":{:.3},\"mb_per_s\":{:.2},\"visible_sha\":\"{}\",\"scrollback\":{}}}",
            corpus.len(),
            secs * 1000.0,
            throughput,
            fnv1a_hex(visible.as_bytes()),
            term.scrollback_len()
        );
        let mut f = fs::File::create(&out).expect("create out");
        f.write_all(json.as_bytes()).expect("write out");
        // Also drop the raw visible grid next to it for eyeball diffing.
        fs::write(format!("{out}.grid.txt"), visible).expect("write grid");
    }
}

/// Deterministic, realistic terminal output: colored build-log style lines,
/// progress bars (CR + erase-to-EOL overwrites), 256-color and truecolor runs,
/// and plenty of newlines to exercise scrollback. Standards-compliant sequences
/// only, so both engines render identically.
fn generate_corpus(target: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(target + 1024);
    let mut rng: u64 = 0x9E37_79B9_7F4A_7C15; // fixed seed → reproducible
    let mut next = || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };

    let words = [
        "Compiling", "orca-core", "Finished", "warning:", "error[E0308]:", "note:",
        "running", "test", "ok", "FAILED", "bytes", "Downloading", "Resolving",
        "Linking", "target", "release", "deps", "fingerprint", "rebuild", "cache",
    ];

    let mut line = 0u64;
    while out.len() < target {
        let r = next();
        match r % 10 {
            0 | 1 => {
                // progress bar: redraw the same line many times with CR + erase
                let pct_steps = 20 + (next() % 30);
                for i in 0..pct_steps {
                    let pct = (i * 100) / pct_steps;
                    out.extend_from_slice(b"\r\x1b[K");
                    out.extend_from_slice(b"\x1b[36m["); // cyan
                    let filled = (pct as usize) * 30 / 100;
                    for _ in 0..filled {
                        out.push(b'#');
                    }
                    for _ in filled..30 {
                        out.push(b'-');
                    }
                    let s = format!("] {pct:3}%  downloading deps");
                    out.extend_from_slice(s.as_bytes());
                    out.extend_from_slice(b"\x1b[0m");
                }
                out.extend_from_slice(b"\r\n");
            }
            2 | 3 | 4 => {
                // colored log line with a couple of SGR runs
                let color = 31 + (next() % 7); // 31..37
                let s = format!("\x1b[{}m{:>11}\x1b[0m ", color, words[(next() as usize) % words.len()]);
                out.extend_from_slice(s.as_bytes());
                let n = 3 + (next() % 8);
                for _ in 0..n {
                    let w = words[(next() as usize) % words.len()];
                    out.extend_from_slice(w.as_bytes());
                    out.push(b' ');
                }
                let s2 = format!("({} ms)\r\n", next() % 5000);
                out.extend_from_slice(s2.as_bytes());
            }
            5 => {
                // truecolor gradient run
                for k in 0..40u32 {
                    let s = format!("\x1b[38;2;{};{};{}m#", (k * 6) % 256, (k * 3) % 256, 200 - (k % 200));
                    out.extend_from_slice(s.as_bytes());
                }
                out.extend_from_slice(b"\x1b[0m\r\n");
            }
            6 => {
                // 256-color palette row
                for k in 0..50u32 {
                    let s = format!("\x1b[48;5;{}m \x1b[0m", 16 + (k % 200));
                    out.extend_from_slice(s.as_bytes());
                }
                out.extend_from_slice(b"\r\n");
            }
            7 => {
                // bold/underline/italic attributes
                let s = format!(
                    "\x1b[1mbold\x1b[0m \x1b[4munderline\x1b[0m \x1b[3mitalic\x1b[0m line {line}\r\n"
                );
                out.extend_from_slice(s.as_bytes());
            }
            _ => {
                // plain text line, varied length
                let n = 20 + (next() % 90);
                for i in 0..n {
                    out.push(b'a' + ((line + i) % 26) as u8);
                }
                out.extend_from_slice(b"\r\n");
            }
        }
        line += 1;
    }
    out
}

/// Small, dependency-free hash so both engines can fingerprint the visible grid.
/// FNV-1a (64-bit) — matched byte-for-byte by the Node side.
fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}
