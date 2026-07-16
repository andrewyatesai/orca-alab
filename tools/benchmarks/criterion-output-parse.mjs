/**
 * Parse raw criterion (cargo bench) console output — e.g. aterm's
 * `cargo bench -p aterm-bench --bench engine_throughput` — into
 * { '<group>/<bench>': { medianMs, mibPerSec } }.
 *
 * Criterion prints the bench id either on the `time:` line or on its own line
 * directly above it; in both layouts the last token before `time:` is the id.
 * Times are normalized to ms, throughputs to MiB/s.
 */

const TIME_RE =
  /([\w./-]+\/[\w./ -]*?)\s+time:\s+\[\s*([\d.]+)\s+(ns|µs|us|ms|s)\s+([\d.]+)\s+(ns|µs|us|ms|s)\s+([\d.]+)\s+(ns|µs|us|ms|s)\s*\]\s*(?:thrpt:\s+\[\s*([\d.]+)\s+(KiB\/s|MiB\/s|GiB\/s)\s+([\d.]+)\s+(KiB\/s|MiB\/s|GiB\/s)\s+([\d.]+)\s+(KiB\/s|MiB\/s|GiB\/s)\s*\])?/g

function toMs(value, unit) {
  switch (unit) {
    case 'ns':
      return value / 1e6
    case 'µs':
    case 'us':
      return value / 1e3
    case 's':
      return value * 1e3
    default:
      return value
  }
}

function toMiBps(value, unit) {
  switch (unit) {
    case 'KiB/s':
      return value / 1024
    case 'GiB/s':
      return value * 1024
    default:
      return value
  }
}

export function parseCriterionOutput(text) {
  const benches = {}
  for (const match of text.matchAll(TIME_RE)) {
    const [, name, , , mid, midUnit, , , , , thrptMid, thrptMidUnit] = match
    benches[name.trim()] = {
      // Criterion's triple is [low, ESTIMATE, high] — the middle is the estimate.
      medianMs: toMs(Number(mid), midUnit),
      mibPerSec: thrptMid ? toMiBps(Number(thrptMid), thrptMidUnit) : null
    }
  }
  return benches
}
