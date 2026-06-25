// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! The APP-FED metric store: a process-global, bounded set of named numeric streams
//! that any process in an aterm window can push to over the control socket
//! (`aterm-ctl metric <name> <value>`), displayed by the app-fed HUD panel. This is
//! how an AI tool reports input/output token spend, a build reports progress, etc.
//! — accurate per-app numbers the OS can't attribute (see `sysmetrics`).
//!
//! Single App/event-loop per process (like `crate::metrics`), so a plain
//! `OnceLock<Mutex<…>>` suffices — no `Arc` threaded through the control listener.
//! The control thread WRITES via [`record`]; the main thread READS via [`snapshot`]
//! on the present path. Memory is hard-bounded: at most [`MAX_STREAMS`] names, each a
//! drop-oldest ring of [`RING_CAP`] timestamped samples (≈50 KB ceiling).

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Max distinct stream names (a new name past this is DROPPED, never evicting an
/// existing stream — a hostile producer can't flush a legitimate one).
const MAX_STREAMS: usize = 32;
/// Samples retained per stream (drop-oldest).
const RING_CAP: usize = 64;
/// Window over which a counter's rate (value/sec) is derived.
const RATE_WINDOW: Duration = Duration::from_secs(5);
/// Samples older than this are ignored on read, so an idle stream's rate decays to 0.
const TTL: Duration = Duration::from_secs(30);

struct Ring {
    samples: VecDeque<(Instant, f64)>,
}

static STORE: OnceLock<Mutex<HashMap<String, Ring>>> = OnceLock::new();

fn store() -> &'static Mutex<HashMap<String, Ring>> {
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Record one sample for `name`. Called on the control thread from the `metric`
/// verb. Bounded: drops a brand-new name once `MAX_STREAMS` is reached.
pub(crate) fn record(name: &str, value: f64, now: Instant) {
    let mut m = store()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !m.contains_key(name) && m.len() >= MAX_STREAMS {
        return;
    }
    let r = m.entry(name.to_string()).or_insert_with(|| Ring {
        samples: VecDeque::with_capacity(RING_CAP),
    });
    r.samples.push_back((now, value));
    while r.samples.len() > RING_CAP {
        r.samples.pop_front();
    }
}

/// A render-ready view of one stream: latest value, derived per-second rate (for
/// monotone counters), and an auto-scaled sparkline of inter-sample throughput.
pub(crate) struct StreamView {
    pub name: String,
    pub last: f64,
    pub rate: f64,
    pub spark: Vec<u8>,
}

/// Snapshot all live streams (sorted by name) for the app-fed panel. Stale samples
/// (older than [`TTL`]) are ignored so idle streams read honestly.
pub(crate) fn snapshot(now: Instant, spark_width: usize) -> Vec<StreamView> {
    let m = store()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut out: Vec<StreamView> = Vec::new();
    for (name, r) in m.iter() {
        let live: Vec<(Instant, f64)> = r
            .samples
            .iter()
            .copied()
            .filter(|(t, _)| now.checked_duration_since(*t).is_some_and(|a| a <= TTL))
            .collect();
        let Some(&newest) = live.last() else {
            continue;
        };
        // Rate from a monotone counter: Δvalue over the window (clamp negative on a
        // counter reset). For non-counter gauges this still reads as "recent slope".
        let oldest = live
            .iter()
            .find(|(t, _)| {
                now.checked_duration_since(*t)
                    .is_some_and(|a| a <= RATE_WINDOW)
            })
            .copied()
            .unwrap_or(newest);
        let dt = newest
            .0
            .checked_duration_since(oldest.0)
            .map_or(0.0, |d| d.as_secs_f64());
        let rate = if dt > 0.0 {
            ((newest.1 - oldest.1) / dt).max(0.0)
        } else {
            0.0
        };
        // Sparkline of inter-sample deltas (per-tick throughput).
        let deltas: Vec<f64> = live
            .windows(2)
            .map(|w| (w[1].1 - w[0].1).max(0.0))
            .collect();
        let spark = crate::hud_bar::levels_autoscaled(&deltas, 1.0, spark_width);
        out.push(StreamView {
            name: name.clone(),
            last: newest.1,
            rate,
            spark,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_derives_rate_and_caps_streams() {
        let t0 = Instant::now();
        // a monotone counter at +100/s over 2s
        for i in 0..=4 {
            record(
                "test.tokens",
                (i * 50) as f64,
                t0 + Duration::from_millis(i as u64 * 500),
            );
        }
        let now = t0 + Duration::from_millis(2000);
        let views = snapshot(now, 8);
        let v = views
            .iter()
            .find(|v| v.name == "test.tokens")
            .expect("stream present");
        assert_eq!(v.last, 200.0);
        assert!((v.rate - 100.0).abs() < 1.0, "rate ~100/s, got {}", v.rate);

        // stream cap: a new name past MAX_STREAMS is dropped.
        for i in 0..(MAX_STREAMS + 10) {
            record(&format!("cap.{i}"), 1.0, now);
        }
        let m = store().lock().unwrap();
        assert!(
            m.len() <= MAX_STREAMS + 1,
            "stream count bounded, got {}",
            m.len()
        );
    }
}
