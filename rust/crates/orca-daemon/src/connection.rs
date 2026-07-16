//! One accepted socket: read the `hello`, then serve either the control (RPC) or
//! the stream (events) role for its client. Faithful to the Node daemon, where
//! each client opens a control socket and a stream socket correlated by clientId.

use crate::protocol::{
    hello_err, hello_ok, parse_hello, MIN_SUPPORTED_PROTOCOL_VERSION, PROTOCOL_VERSION,
};
use crate::registry::Registry;
use crate::rpc::dispatch_request;
use orca_net::{encode_ndjson_line, NdjsonEvent, NdjsonSplitter, NDJSON_MAX_LINE_BYTES};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread;

/// A daemon transport stream: readable, writable, movable onto worker threads, and
/// cloneable into an independent reader/writer pair. Implemented by `UnixStream`
/// (macOS/Linux) and the Windows named-pipe stream, so all the connection logic
/// below — hello handshake, control RPC loop, stream fan-out — is transport-
/// agnostic and each platform only supplies the socket type.
pub trait DaemonStream: Read + Write + Send + 'static {
    fn try_clone_stream(&self) -> io::Result<Self>
    where
        Self: Sized;
}

#[cfg(unix)]
impl DaemonStream for std::os::unix::net::UnixStream {
    fn try_clone_stream(&self) -> io::Result<Self> {
        self.try_clone()
    }
}

#[cfg(windows)]
impl DaemonStream for orca_winpipe::NamedPipeStream {
    fn try_clone_stream(&self) -> io::Result<Self> {
        self.try_clone()
    }
}

/// Fire-and-forget request ids carry this prefix (types.ts `NOTIFY_PREFIX`); the
/// Node daemon writes no response for them, so neither do we (else every keystroke
/// `write` would echo a useless `{id:'notify_N',ok:true}` line on the control socket).
const NOTIFY_PREFIX: &str = "notify_";

/// Blocking NDJSON line reader over a socket: feeds raw chunks through the shared
/// splitter and yields whole lines. A UTF-8 char straddling a read boundary is
/// reassembled (not corrupted) by carrying its incomplete tail across reads, so a
/// >64KB non-ASCII `write`/paste survives the transport byte-exact.
struct LineReader<S> {
    stream: S,
    splitter: NdjsonSplitter,
    pending: VecDeque<String>,
    /// Reused read scratch — allocated once per connection, not re-zeroed per read.
    buf: Vec<u8>,
    /// The incomplete trailing multibyte UTF-8 sequence (≤ 3 bytes) from the previous
    /// read, prepended to the next one so a char split at a 64KB boundary isn't lost.
    carry: Vec<u8>,
}

impl<S: Read> LineReader<S> {
    fn new(stream: S) -> Self {
        Self {
            stream,
            splitter: NdjsonSplitter::new(NDJSON_MAX_LINE_BYTES),
            pending: VecDeque::new(),
            buf: vec![0u8; 65536],
            carry: Vec::new(),
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
            // Decode carrying any partial multibyte char across the read boundary
            // BEFORE the splitter frames on '\n' (0x0A never occurs inside a multibyte
            // sequence, so line-splitting the decoded text stays correct). Feed into a
            // scratch Vec so the borrow of `self.buf`/`self.carry` ends before the
            // &mut-self splitter call.
            let mut events = Vec::new();
            let chunk = decode_streaming(&mut self.carry, &self.buf[..n]);
            self.splitter.feed(chunk.as_ref(), &mut events);
            drop(chunk);
            for event in events {
                if let NdjsonEvent::Line(line) = event {
                    self.pending.push_back(line);
                }
            }
        }
    }
}

/// Decode `bytes` as UTF-8, carrying an incomplete trailing multibyte sequence in
/// `carry` across calls so a char straddling a read boundary is reassembled instead
/// of corrupted to U+FFFD — the fix for a >64KB non-ASCII write split across reads.
/// A genuinely invalid byte (not a boundary split) is lossy-replaced, matching the
/// prior behavior for malformed input. `carry` never exceeds 3 bytes (a max-length
/// incomplete UTF-8 tail), so it adds no unbounded growth and the splitter's OOM
/// budget invariant is untouched. The all-valid, nothing-carried case borrows with
/// zero allocation, preserving the hot-path cost.
fn decode_streaming<'a>(carry: &mut Vec<u8>, bytes: &'a [u8]) -> Cow<'a, str> {
    if carry.is_empty() {
        return match std::str::from_utf8(bytes) {
            Ok(s) => Cow::Borrowed(s),
            // Incomplete trailing char (error at end, ≤ 3 bytes): emit the valid
            // prefix, carry the tail for the next read.
            Err(e) if e.error_len().is_none() && bytes.len() - e.valid_up_to() <= 3 => {
                let vut = e.valid_up_to();
                *carry = bytes[vut..].to_vec();
                Cow::Borrowed(std::str::from_utf8(&bytes[..vut]).expect("valid_up_to is a boundary"))
            }
            // A real mid-stream invalid byte — lossy-replace as before (rare).
            Err(_) => Cow::Owned(String::from_utf8_lossy(bytes).into_owned()),
        };
    }
    // A tail was carried: prepend it, then decode the joined bytes.
    let mut combined = std::mem::take(carry);
    combined.extend_from_slice(bytes);
    match std::str::from_utf8(&combined) {
        Ok(_) => Cow::Owned(String::from_utf8(combined).expect("checked valid")),
        Err(e) if e.error_len().is_none() && combined.len() - e.valid_up_to() <= 3 => {
            let vut = e.valid_up_to();
            let good = std::str::from_utf8(&combined[..vut]).expect("valid_up_to is a boundary");
            let good = good.to_string();
            *carry = combined[vut..].to_vec();
            Cow::Owned(good)
        }
        Err(_) => Cow::Owned(String::from_utf8_lossy(&combined).into_owned()),
    }
}

pub fn handle_connection<S: DaemonStream>(
    stream: S,
    registry: Arc<Registry>,
    expected_token: Option<Arc<str>>,
) {
    let Ok(mut writer) = stream.try_clone_stream() else {
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
    // v1019 is additive over v1018 (subscribe/unsubscribe only), so both hellos
    // are accepted — a pre-subscriber client keeps its full behavior.
    if hello.version != PROTOCOL_VERSION && hello.version != MIN_SUPPORTED_PROTOCOL_VERSION {
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

fn serve_control<S: Read + Write>(
    reader: &mut LineReader<S>,
    writer: &mut S,
    registry: &Arc<Registry>,
    client_id: &str,
) {
    while let Some(line) = reader.next_line() {
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        // A notify (write/resize/etc.) is fire-and-forget: dispatch its side effects
        // but suppress the response line, matching the Node daemon. Its dispatch can
        // still push events (e.g. a synthetic exit) onto the client's stream socket.
        let is_notify = request
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id.starts_with(NOTIFY_PREFIX));
        let response = dispatch_request(&request, registry, client_id);
        if is_notify {
            continue;
        }
        if writer
            .write_all(encode_ndjson_line(&response).as_bytes())
            .is_err()
        {
            break;
        }
    }
}

fn serve_stream<S: DaemonStream>(
    reader: &mut LineReader<S>,
    mut writer: S,
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
    // A dropped follower must not linger as a fan-out target: its subscriptions
    // die with its stream. Owners (and other subscribers) are untouched.
    registry.remove_subscriber_from_all(&client_id);
    let _ = drain.join();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_chunk_borrows_with_zero_alloc() {
        let mut carry = Vec::new();
        let out = decode_streaming(&mut carry, "hello €".as_bytes());
        assert_eq!(out.as_ref(), "hello €");
        assert!(carry.is_empty());
        assert!(matches!(out, Cow::Borrowed(_)), "an all-valid chunk borrows (no alloc)");
    }

    #[test]
    fn multibyte_char_split_across_two_reads_reassembles() {
        // '€' = E2 82 AC. A read boundary falls after the lead byte.
        let mut carry = Vec::new();
        let first = decode_streaming(&mut carry, &[0xE2]);
        assert_eq!(first.as_ref(), "", "no complete char yet");
        assert_eq!(carry, vec![0xE2u8], "the incomplete lead byte is carried");
        let second = decode_streaming(&mut carry, &[0x82, 0xAC]);
        assert_eq!(second.as_ref(), "€", "the char is reassembled across the boundary");
        assert!(carry.is_empty());
    }

    #[test]
    fn multibyte_char_split_one_byte_per_read() {
        let mut carry = Vec::new();
        assert_eq!(decode_streaming(&mut carry, &[0xE2]).as_ref(), "");
        assert_eq!(decode_streaming(&mut carry, &[0x82]).as_ref(), "");
        assert_eq!(decode_streaming(&mut carry, &[0xAC]).as_ref(), "€");
        assert!(carry.is_empty());
    }

    #[test]
    fn valid_prefix_then_incomplete_tail_carries_only_the_tail() {
        let mut carry = Vec::new();
        let out = decode_streaming(&mut carry, &[b'a', 0xE2, 0x82]); // "a" + partial '€'
        assert_eq!(out.as_ref(), "a");
        assert_eq!(carry, vec![0xE2u8, 0x82], "only the incomplete tail is carried");
        assert_eq!(decode_streaming(&mut carry, &[0xAC]).as_ref(), "€");
    }

    #[test]
    fn carried_tail_then_trailing_ascii() {
        let mut carry = vec![0xE2u8, 0x82]; // partial '€'
        let out = decode_streaming(&mut carry, &[0xAC, b'x', b'y']);
        assert_eq!(out.as_ref(), "€xy");
        assert!(carry.is_empty());
    }

    #[test]
    fn genuinely_invalid_byte_is_lossy_not_carried() {
        let mut carry = Vec::new();
        let out = decode_streaming(&mut carry, &[b'a', 0xFF, b'b']); // 0xFF never valid
        assert!(out.contains('\u{FFFD}'), "a real invalid byte is lossy-replaced: {out:?}");
        assert!(carry.is_empty(), "invalid (non-boundary) bytes are not carried");
    }

    /// End-to-end: a JSON line whose multibyte char straddles the read boundary must
    /// frame intact through the splitter — the actual bug (a >64KB non-ASCII write
    /// used to corrupt the char at the boundary before framing).
    #[test]
    fn line_split_mid_char_frames_intact_through_the_splitter() {
        let mut carry = Vec::new();
        let mut splitter = NdjsonSplitter::new(NDJSON_MAX_LINE_BYTES);
        let mut events = Vec::new();
        let full = "{\"d\":\"€\"}\n".as_bytes().to_vec();
        let cut = full.iter().position(|&b| b == 0xE2).unwrap() + 1; // mid-'€'
        let c1 = decode_streaming(&mut carry, &full[..cut]);
        splitter.feed(c1.as_ref(), &mut events);
        drop(c1);
        let c2 = decode_streaming(&mut carry, &full[cut..]);
        splitter.feed(c2.as_ref(), &mut events);
        drop(c2);
        let lines: Vec<_> = events
            .into_iter()
            .filter_map(|e| match e {
                NdjsonEvent::Line(l) => Some(l),
                NdjsonEvent::Oversized { .. } => None,
            })
            .collect();
        assert_eq!(lines, ["{\"d\":\"€\"}"], "the split-char line frames byte-exact");
    }
}
