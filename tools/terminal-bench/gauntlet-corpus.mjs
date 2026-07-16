// The parity-corpus ratchet — the `corpus` gauntlet axis (moonshot F2 "publish
// metrics" half). The machine-checked behavioral parity case count may only GROW;
// a drop means a corpus was deleted/shrunk (a regression in the TS↔Rust equivalence
// net) and FAILs. Delegates the count + baseline compare to the metrics tool
// (tools/parity-corpus-metrics.mjs --check), which is also the human-facing
// `pnpm corpus:metrics` / `corpus:check`.
//
// Extracted from gauntlet.mjs to keep that file under its max-lines cap; the host
// passes in the shared primitives (repo root, sh).

import { join } from 'node:path'

export function corpusGate({ repo, sh }) {
  let out
  let code = 0
  try {
    out = sh('node', [join(repo, 'tools', 'parity-corpus-metrics.mjs'), '--check', '--json'])
  } catch (e) {
    code = e.status ?? 1
    out = `${e.stdout ?? ''}`
  }
  let report
  try {
    report = JSON.parse(out)
  } catch {
    // exit 3 = no baseline yet (non-JSON message); surface as REVIEW to generate it.
    return code === 3
      ? {
          status: 'REVIEW',
          detail:
            'no parity-corpus baseline — run: node tools/parity-corpus-metrics.mjs --write-baseline'
        }
      : { status: 'FAIL', detail: `parity-corpus-metrics emitted no JSON (exit ${code})` }
  }
  return {
    status: report.status,
    metrics: report.current,
    detail: report.shrank.length
      ? `parity coverage SHRANK: ${report.shrank.join(' · ')}`
      : `${report.current.totalCases} machine-checked parity cases${report.grew.length ? ` (grew: ${report.grew.join(', ')} — bump baseline)` : ''}`
  }
}
