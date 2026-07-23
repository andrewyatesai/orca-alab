//! Writer-side semantic stream queue (daemon-pty-drain-investigation.md, "the
//! correct design"): `route_output` binds recipients per PTY read and enqueues
//! SEMANTIC items; each stream socket's writer thread owns encoding and
//! coalesces adjacent same-session output to ~32 KiB per socket write. One
//! emitter per socket → order-safe; no pump-side batching → no reattach
//! snapshot/stream duplication; coalescing only merges items ALREADY queued
//! (`try_recv`), so an interactive keystroke's echo flushes immediately.

use crate::bounded_stream_channel::StreamReceiver;
use crate::protocol::{data_event, data_frame, event_frame};
use orca_net::encode_ndjson_line;
use std::io::Write;

/// One semantic stream-plane message, queued per client. Encoding happens in
/// the writer thread (per the negotiated wire format), not at enqueue time.
pub enum StreamItem {
    /// One session's PTY output text. The writer may coalesce ADJACENT items of
    /// the SAME session; cross-item order is never changed.
    Data { session_id: String, text: String },
    /// A control event (exit today) as its JSON text. Never coalesced, and all
    /// pending data flushes before it.
    Event { json: String },
}

/// A stream socket's negotiated wire format (v1020 hello `streamFormat`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StreamWireFormat {
    Ndjson,
    Binary,
}

/// Coalescing cap per socket write. ~32 KiB measured optimal for the cat-flood
/// (daemon-pty-drain-investigation.md: 156→248 MB/s, flat beyond 32); one item
/// may overshoot it by at most one PTY read (≤64 KiB), still far under the
/// 16 MiB NDJSON line cap even at worst-case 6× JSON escape expansion.
pub const STREAM_COALESCE_MAX_BYTES: usize = 32 * 1024;

/// Encode one (possibly coalesced) item in the socket's negotiated format —
/// exactly the bytes the pre-semantic-queue daemon wrote per chunk, so the
/// wire is indistinguishable apart from data-chunk boundaries.
pub fn encode_stream_item(item: &StreamItem, format: StreamWireFormat) -> Vec<u8> {
    match (item, format) {
        (StreamItem::Data { session_id, text }, StreamWireFormat::Ndjson) => {
            encode_ndjson_line(&data_event(session_id, text)).into_bytes()
        }
        (StreamItem::Data { session_id, text }, StreamWireFormat::Binary) => {
            data_frame(session_id, text)
        }
        (StreamItem::Event { json }, StreamWireFormat::Ndjson) => {
            encode_ndjson_line(json).into_bytes()
        }
        (StreamItem::Event { json }, StreamWireFormat::Binary) => event_frame(json),
    }
}

/// The stream writer loop: block while idle, then write each item, merging
/// adjacent same-session data already queued up to `STREAM_COALESCE_MAX_BYTES`.
/// Returns when every sender is dropped (client teardown) or a write fails
/// (socket closed) — matching the old per-frame drain's exit conditions.
pub fn drain_stream_items<W: Write>(
    writer: &mut W,
    rx: &StreamReceiver,
    format: StreamWireFormat,
) {
    // An item pulled while coalescing that must NOT merge (an event, or another
    // session's data) is carried to the next turn — never reordered or dropped.
    let mut carried: Option<StreamItem> = None;
    loop {
        let item = match carried.take() {
            Some(item) => item,
            None => match rx.recv() {
                Ok(item) => item,
                // Disconnected with nothing carried: every queued item was
                // already written (try_recv drains before recv blocks).
                Err(_) => return,
            },
        };
        let item = match item {
            StreamItem::Data { session_id, mut text } => {
                // try_recv-only coalescing: an EMPTY queue flushes immediately
                // (zero added latency for interactive output); only a backed-up
                // flood — where the socket write is the bottleneck anyway —
                // merges reads into bigger frames.
                while text.len() < STREAM_COALESCE_MAX_BYTES {
                    match rx.try_recv() {
                        Ok(StreamItem::Data {
                            session_id: next_sid,
                            text: next_text,
                        }) if next_sid == session_id => text.push_str(&next_text),
                        Ok(other) => {
                            carried = Some(other);
                            break;
                        }
                        // Empty or disconnected: flush what we have.
                        Err(_) => break,
                    }
                }
                StreamItem::Data { session_id, text }
            }
            event => event,
        };
        if writer.write_all(&encode_stream_item(&item, format)).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bounded_stream_channel::stream_channel;
    use crate::protocol::{exit_event, FRAME_HEADER_SIZE, FRAME_TYPE_DATA, FRAME_TYPE_EVENT};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// Records each `write_all` as one entry, so tests can assert exactly how
    /// items were coalesced into socket writes.
    #[derive(Clone, Default)]
    struct WriteLog(Arc<Mutex<Vec<Vec<u8>>>>);

    impl WriteLog {
        fn writes(&self) -> Vec<Vec<u8>> {
            self.0.lock().unwrap().clone()
        }
    }

    impl Write for WriteLog {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().push(buf.to_vec());
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn data(sid: &str, text: &str) -> StreamItem {
        StreamItem::Data {
            session_id: sid.to_string(),
            text: text.to_string(),
        }
    }

    /// Decode a binary Data frame write back to (session_id, payload text).
    fn decode_data_frame(bytes: &[u8]) -> (String, String) {
        assert_eq!(bytes[0], FRAME_TYPE_DATA);
        let sid_len = bytes[FRAME_HEADER_SIZE] as usize;
        let sid_end = FRAME_HEADER_SIZE + 1 + sid_len;
        (
            String::from_utf8(bytes[FRAME_HEADER_SIZE + 1..sid_end].to_vec()).unwrap(),
            String::from_utf8(bytes[sid_end..].to_vec()).unwrap(),
        )
    }

    #[test]
    fn adjacent_same_session_items_coalesce_into_one_write() {
        let (tx, rx) = stream_channel();
        tx.send(data("s", "one ")).unwrap();
        tx.send(data("s", "two ")).unwrap();
        tx.send(data("s", "three")).unwrap();
        drop(tx);
        let mut log = WriteLog::default();
        drain_stream_items(&mut log, &rx, StreamWireFormat::Binary);
        let writes = log.writes();
        assert_eq!(writes.len(), 1, "three queued chunks → one socket write");
        assert_eq!(
            decode_data_frame(&writes[0]),
            ("s".to_string(), "one two three".to_string()),
            "payloads concatenate in queue order"
        );
    }

    #[test]
    fn event_never_merges_and_pending_data_flushes_before_it() {
        let (tx, rx) = stream_channel();
        tx.send(data("s", "before-a ")).unwrap();
        tx.send(data("s", "before-b")).unwrap();
        tx.send(StreamItem::Event {
            json: exit_event("s", 0),
        })
        .unwrap();
        tx.send(data("s", "after")).unwrap();
        drop(tx);
        let mut log = WriteLog::default();
        drain_stream_items(&mut log, &rx, StreamWireFormat::Binary);
        let writes = log.writes();
        assert_eq!(writes.len(), 3, "data-before / event / data-after");
        assert_eq!(decode_data_frame(&writes[0]).1, "before-a before-b");
        assert_eq!(writes[1][0], FRAME_TYPE_EVENT, "the exit flushes AFTER prior data");
        assert_eq!(
            &writes[1][FRAME_HEADER_SIZE..],
            exit_event("s", 0).as_bytes()
        );
        assert_eq!(
            decode_data_frame(&writes[2]).1,
            "after",
            "data queued after the event stays after it"
        );
    }

    #[test]
    fn cross_session_data_never_merges_and_keeps_order() {
        let (tx, rx) = stream_channel();
        tx.send(data("s1", "a1")).unwrap();
        tx.send(data("s2", "b1")).unwrap();
        tx.send(data("s1", "a2")).unwrap();
        drop(tx);
        let mut log = WriteLog::default();
        drain_stream_items(&mut log, &rx, StreamWireFormat::Binary);
        let decoded: Vec<_> = log.writes().iter().map(|w| decode_data_frame(w)).collect();
        assert_eq!(
            decoded,
            vec![
                ("s1".to_string(), "a1".to_string()),
                ("s2".to_string(), "b1".to_string()),
                ("s1".to_string(), "a2".to_string()),
            ],
            "another session's item is a coalescing barrier, in exact queue order"
        );
    }

    #[test]
    fn coalescing_caps_near_32kib() {
        let chunk = "x".repeat(10 * 1024);
        let (tx, rx) = stream_channel();
        for _ in 0..5 {
            tx.send(data("s", &chunk)).unwrap();
        }
        drop(tx);
        let mut log = WriteLog::default();
        drain_stream_items(&mut log, &rx, StreamWireFormat::Binary);
        let sizes: Vec<usize> = log
            .writes()
            .iter()
            .map(|w| decode_data_frame(w).1.len())
            .collect();
        // 10 KiB chunks: 30 KiB is still under the cap so a 4th merges (40 KiB —
        // bounded overshoot), then the remaining chunk flushes alone.
        assert_eq!(sizes, vec![40 * 1024, 10 * 1024]);
    }

    #[test]
    fn single_item_flushes_immediately_while_channel_stays_open() {
        let (tx, rx) = stream_channel();
        let log = WriteLog::default();
        let mut writer = log.clone();
        let drain =
            std::thread::spawn(move || drain_stream_items(&mut writer, &rx, StreamWireFormat::Binary));
        // The sender stays open: only try_recv-empty (not disconnect) can flush.
        tx.send(data("s", "echo")).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while log.writes().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            log.writes().len(),
            1,
            "a lone interactive chunk is written without waiting for more input"
        );
        assert_eq!(decode_data_frame(&log.writes()[0]).1, "echo");
        drop(tx);
        drain.join().unwrap();
    }

    #[test]
    fn sender_dropped_mid_queue_still_flushes_the_tail() {
        let (tx, rx) = stream_channel();
        tx.send(data("s", "tail-a")).unwrap();
        tx.send(data("s", "tail-b")).unwrap();
        drop(tx); // disconnect BEFORE the drain runs
        let mut log = WriteLog::default();
        drain_stream_items(&mut log, &rx, StreamWireFormat::Binary);
        assert_eq!(decode_data_frame(&log.writes()[0]).1, "tail-atail-b");
    }

    #[test]
    fn ndjson_encoding_matches_the_legacy_per_chunk_wire() {
        let line = encode_stream_item(&data("sess", "hi\x1b[0m"), StreamWireFormat::Ndjson);
        assert_eq!(
            String::from_utf8(line).unwrap(),
            encode_ndjson_line(&data_event("sess", "hi\x1b[0m")),
            "a data item encodes to the exact pre-queue NDJSON line"
        );
        let exit = StreamItem::Event {
            json: exit_event("sess", 3),
        };
        assert_eq!(
            String::from_utf8(encode_stream_item(&exit, StreamWireFormat::Ndjson)).unwrap(),
            encode_ndjson_line(&exit_event("sess", 3))
        );
        assert_eq!(
            encode_stream_item(&exit, StreamWireFormat::Binary),
            event_frame(&exit_event("sess", 3))
        );
    }

    #[test]
    fn ndjson_drain_coalesces_and_flushes_before_events_like_binary() {
        let (tx, rx) = stream_channel();
        tx.send(data("s", "a")).unwrap();
        tx.send(data("s", "b")).unwrap();
        tx.send(StreamItem::Event {
            json: exit_event("s", 0),
        })
        .unwrap();
        tx.send(data("s", "c")).unwrap();
        drop(tx);
        let mut log = WriteLog::default();
        drain_stream_items(&mut log, &rx, StreamWireFormat::Ndjson);
        let lines: Vec<String> = log
            .writes()
            .iter()
            .map(|w| String::from_utf8(w.clone()).unwrap())
            .collect();
        assert_eq!(
            lines,
            vec![
                encode_ndjson_line(&data_event("s", "ab")),
                encode_ndjson_line(&exit_event("s", 0)),
                encode_ndjson_line(&data_event("s", "c")),
            ],
            "NDJSON: adjacent data coalesces into ONE line, pending data flushes before the event, post-event data stays after"
        );
    }

    #[test]
    fn write_error_ends_the_drain_without_panicking() {
        struct FailingWriter;
        impl Write for FailingWriter {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "closed"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let (tx, rx) = stream_channel();
        tx.send(data("s", "x")).unwrap();
        tx.send(StreamItem::Event {
            json: exit_event("s", 0),
        })
        .unwrap();
        drop(tx);
        drain_stream_items(&mut FailingWriter, &rx, StreamWireFormat::Ndjson);
    }
}
