//! One accepted socket: read the `hello`, then serve either the control (RPC) or
//! the stream (events) role for its client. Faithful to the Node daemon, where
//! each client opens a control socket and a stream socket correlated by clientId.

use crate::protocol::{hello_err, hello_ok, parse_hello, PROTOCOL_VERSION};
use crate::registry::Registry;
use crate::rpc::dispatch_request;
use orca_net::{encode_ndjson_line, NdjsonEvent, NdjsonSplitter, NDJSON_MAX_LINE_BYTES};
use serde_json::Value;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread;

/// Blocking NDJSON line reader over a socket: feeds raw chunks through the shared
/// splitter and yields whole lines. NOTE: chunks are decoded lossy-UTF-8, so a
/// multibyte char split across a read boundary is corrupted; requests are NDJSON
/// and rarely large, but carrying a partial-multibyte tail across reads (like the
/// Node StringDecoder) is a tracked follow-up.
struct LineReader {
    stream: UnixStream,
    splitter: NdjsonSplitter,
    pending: VecDeque<String>,
    /// Reused read scratch — allocated once per connection, not re-zeroed per read.
    buf: Vec<u8>,
}

impl LineReader {
    fn new(stream: UnixStream) -> Self {
        Self {
            stream,
            splitter: NdjsonSplitter::new(NDJSON_MAX_LINE_BYTES),
            pending: VecDeque::new(),
            buf: vec![0u8; 65536],
        }
    }

    fn next_line(&mut self) -> Option<String> {
        loop {
            if let Some(line) = self.pending.pop_front() {
                return Some(line);
            }
            let n = self.stream.read(&mut self.buf).ok()?;
            if n == 0 {
                return None; // peer closed
            }
            let chunk = String::from_utf8_lossy(&self.buf[..n]);
            for event in self.splitter.feed_collect(chunk.as_ref()) {
                if let NdjsonEvent::Line(line) = event {
                    self.pending.push_back(line);
                }
            }
        }
    }
}

pub fn handle_connection(
    stream: UnixStream,
    registry: Arc<Registry>,
    expected_token: Option<Arc<str>>,
) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = LineReader::new(stream);

    let Some(first) = reader.next_line() else {
        return;
    };
    let parsed = serde_json::from_str::<Value>(&first).ok();
    let Some(hello) = parsed.as_ref().and_then(parse_hello) else {
        let _ = writer.write_all(encode_ndjson_line(&hello_err("Expected hello")).as_bytes());
        return;
    };
    if hello.version != PROTOCOL_VERSION {
        let _ = writer
            .write_all(encode_ndjson_line(&hello_err("Protocol version mismatch")).as_bytes());
        return;
    }
    // Token gate (order matches the Node daemon: version → token → ok). Skipped
    // when no token is configured (standalone / parity harness).
    if let Some(expected) = expected_token.as_deref() {
        if hello.token != expected {
            let _ = writer.write_all(encode_ndjson_line(&hello_err("Invalid token")).as_bytes());
            return;
        }
    }
    if writer
        .write_all(encode_ndjson_line(&hello_ok()).as_bytes())
        .is_err()
    {
        return;
    }

    match hello.role.as_str() {
        "control" => serve_control(&mut reader, &mut writer, &registry, &hello.client_id),
        "stream" => serve_stream(&mut reader, writer, &registry, hello.client_id),
        _ => {}
    }
}

fn serve_control(
    reader: &mut LineReader,
    writer: &mut UnixStream,
    registry: &Arc<Registry>,
    client_id: &str,
) {
    while let Some(line) = reader.next_line() {
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let response = dispatch_request(&request, registry, client_id);
        if writer
            .write_all(encode_ndjson_line(&response).as_bytes())
            .is_err()
        {
            break;
        }
    }
}

fn serve_stream(
    reader: &mut LineReader,
    mut writer: UnixStream,
    registry: &Arc<Registry>,
    client_id: String,
) {
    let (tx, rx) = channel::<String>();
    // Drain queued events to the socket on a dedicated thread so the read side can
    // block on close detection independently.
    let drain = thread::spawn(move || {
        while let Ok(line) = rx.recv() {
            if writer.write_all(line.as_bytes()).is_err() {
                break;
            }
        }
    });
    // Install the sender. Detached-while-idle output isn't buffered for raw replay —
    // the reattach snapshot (built from the engine) restores state instead.
    registry.register_stream(client_id.clone(), tx);
    // A stream socket is daemon→client; the client rarely sends. Block until it
    // closes, then tear down — dropping the registry's sender ends the drain thread.
    while reader.next_line().is_some() {}
    registry.unregister_stream(&client_id);
    let _ = drain.join();
}
