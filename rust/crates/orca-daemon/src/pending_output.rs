//! The per-session incremental-checkpoint log (`session.ts` pendingOutput*). Output,
//! resize, and clear events accumulate as typed records between `takePendingOutput`
//! ticks; the client appends each drained batch (with its monotonic seq) to the
//! on-disk history log so a crash cold-restore can replay them on top of the last
//! full snapshot. Bounded at 2MB: past the cap the batch is dropped and `overflowed`
//! is flagged so the caller falls back to a full-snapshot checkpoint. This is a
//! DISTINCT concern from live streaming — it accumulates whether or not a client is
//! attached, and is drained by the checkpoint driver, not the stream socket.

use serde_json::{json, Value};

/// Matches PENDING_OUTPUT_MAX_BYTES in session.ts. A batch this large JSON-escapes
/// to well under NDJSON_MAX_LINE_BYTES (16MB).
const MAX_BYTES: usize = 2 * 1024 * 1024;
/// Coalesce adjacent output into one record until it reaches this size — TUIs emit
/// thousands of tiny chunks between ticks; coalescing keeps the take + log compact.
const COALESCE_MAX: usize = 64 * 1024;
/// Byte weight charged for a control (resize/clear) record, mirroring session.ts.
const CONTROL_RECORD_BYTES: usize = 8;

enum PendingRecord {
    Output(String),
    Resize { cols: u16, rows: u16 },
    Clear,
}

impl PendingRecord {
    fn to_json(&self) -> Value {
        match self {
            PendingRecord::Output(data) => json!({ "kind": "output", "data": data }),
            PendingRecord::Resize { cols, rows } => {
                json!({ "kind": "resize", "cols": cols, "rows": rows })
            }
            PendingRecord::Clear => json!({ "kind": "clear" }),
        }
    }
}

#[derive(Default)]
pub struct PendingOutput {
    records: Vec<PendingRecord>,
    bytes: usize,
    overflowed: bool,
    seq: u64,
}

impl PendingOutput {
    /// Append PTY output, coalescing into the trailing output record while it is
    /// under the segment cap. No-op once overflowed (until the next drain).
    /// Empty chunks (a decode-carry read, a fully-stripped ready marker) are
    /// skipped — session.ts never records them, and an empty record would just
    /// bloat the on-disk log.
    pub fn record_output(&mut self, data: &str) {
        if data.is_empty() || self.overflowed || self.charge(data.len()) {
            return;
        }
        if let Some(PendingRecord::Output(last)) = self.records.last_mut() {
            if last.len() < COALESCE_MAX {
                last.push_str(data);
                return;
            }
        }
        self.records.push(PendingRecord::Output(data.to_string()));
    }

    pub fn record_resize(&mut self, cols: u16, rows: u16) {
        if self.overflowed || self.charge(CONTROL_RECORD_BYTES) {
            return;
        }
        self.records.push(PendingRecord::Resize { cols, rows });
    }

    pub fn record_clear(&mut self) {
        if self.overflowed || self.charge(CONTROL_RECORD_BYTES) {
            return;
        }
        self.records.push(PendingRecord::Clear);
    }

    /// Charge `bytes` against the cap. Returns true if it overflowed (caller must
    /// then skip the record) — the batch is dropped and flagged so the next take
    /// forces a full-snapshot checkpoint.
    fn charge(&mut self, bytes: usize) -> bool {
        if self.bytes + bytes > MAX_BYTES {
            self.records.clear();
            self.bytes = 0;
            self.overflowed = true;
            return true;
        }
        self.bytes += bytes;
        false
    }

    /// Drain the batch as JSON records with a fresh monotonic seq, resetting the
    /// accumulator. `(records, seq, overflowed)`.
    pub fn take(&mut self) -> (Vec<Value>, u64, bool) {
        let records: Vec<Value> = self.records.drain(..).map(|r| r.to_json()).collect();
        let overflowed = self.overflowed;
        self.bytes = 0;
        self.overflowed = false;
        self.seq += 1;
        (records, self.seq, overflowed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesces_adjacent_output_and_sequences_batches() {
        let mut p = PendingOutput::default();
        p.record_output("ab");
        p.record_output("cd");
        let (records, seq, overflowed) = p.take();
        assert_eq!(
            records.len(),
            1,
            "adjacent output coalesced into one record"
        );
        assert_eq!(records[0]["kind"], json!("output"));
        assert_eq!(records[0]["data"], json!("abcd"));
        assert_eq!(seq, 1);
        assert!(!overflowed);
        // A control record breaks coalescing; seq advances each take.
        p.record_output("x");
        p.record_resize(100, 30);
        p.record_output("y");
        let (records, seq, _) = p.take();
        assert_eq!(records.len(), 3);
        assert_eq!(
            records[1],
            json!({ "kind": "resize", "cols": 100, "rows": 30 })
        );
        assert_eq!(seq, 2);
    }

    #[test]
    fn overflow_drops_the_batch_and_flags_until_drained() {
        let mut p = PendingOutput::default();
        let big = "z".repeat(MAX_BYTES + 1);
        p.record_output(&big);
        p.record_output("more"); // ignored while overflowed
        let (records, _, overflowed) = p.take();
        assert!(records.is_empty(), "overflowed batch is dropped");
        assert!(overflowed, "overflow flagged for the caller");
        // Reset after drain: new output accumulates again.
        p.record_output("fresh");
        let (records, _, overflowed) = p.take();
        assert_eq!(records.len(), 1);
        assert!(!overflowed);
    }
}
