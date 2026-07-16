//! Stream-plane throughput sender for the binary-frames-vs-NDJSON bench
//! (src/main/daemon/daemon-stream-frame-throughput.bench.test.ts).
//!
//! Plays the daemon's PTY pump + stream encode for ONE connection: reads the
//! corpus file, decodes 64KB chunks through the production Utf8StreamDecoder
//! (exactly what pump_output does before route_output), then writes each chunk
//! in the requested wire format — `encode_ndjson_line(data_event(..))` or
//! `data_frame(..)` — followed by a final exit event. No PTY/JSON parsing
//! differences leak in: both modes run the identical read/decode/write loop,
//! so a throughput delta is purely the wire encoding.
//!
//! Usage: stream-throughput-bench <socket_path> <corpus_path> <ndjson|binary> [chunk_bytes]

#[cfg(unix)]
fn main() -> std::io::Result<()> {
    use orca_daemon::protocol::{data_event, data_frame, event_frame, exit_event};
    use orca_daemon::utf8_stream_decoder::Utf8StreamDecoder;
    use orca_net::encode_ndjson_line;
    use std::io::{BufWriter, Write};
    use std::os::unix::net::UnixListener;

    let mut args = std::env::args().skip(1);
    let socket_path = args.next().expect("socket_path");
    let corpus_path = args.next().expect("corpus_path");
    let mode = args.next().expect("mode ndjson|binary");
    let chunk_bytes: usize = args
        .next()
        .map(|s| s.parse().expect("chunk_bytes"))
        .unwrap_or(65536);
    let binary = match mode.as_str() {
        "binary" => true,
        "ndjson" => false,
        other => panic!("unknown mode {other}"),
    };

    let corpus = std::fs::read(&corpus_path)?;
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path)?;
    let (stream, _) = listener.accept()?;
    // Buffer socket writes like the kernel-coalesced daemon socket; identical
    // for both modes, so it cancels out of the comparison.
    let mut writer = BufWriter::with_capacity(256 * 1024, stream);

    let session_id = "bench-session";
    let mut decoder = Utf8StreamDecoder::new();
    for chunk in corpus.chunks(chunk_bytes) {
        let text = decoder.decode(chunk);
        if text.is_empty() {
            continue;
        }
        if binary {
            writer.write_all(&data_frame(session_id, &text))?;
        } else {
            writer.write_all(encode_ndjson_line(&data_event(session_id, &text)).as_bytes())?;
        }
    }
    let exit_json = exit_event(session_id, 0);
    if binary {
        writer.write_all(&event_frame(&exit_json))?;
    } else {
        writer.write_all(encode_ndjson_line(&exit_json).as_bytes())?;
    }
    writer.flush()?;
    Ok(())
}

#[cfg(not(unix))]
fn main() {
    eprintln!("stream-throughput-bench is unix-only");
    std::process::exit(1);
}
