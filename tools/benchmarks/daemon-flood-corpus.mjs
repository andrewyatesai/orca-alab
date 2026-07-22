// Deterministic flood corpus for the daemon flood harness — byte-identical to
// generate_corpus in rust/crates/orca-daemon/examples/stream_flood_bench.rs so
// numbers from the Rust example and this tool are directly comparable.

import { createWriteStream, existsSync, statSync } from 'node:fs'
import os from 'node:os'
import { join } from 'node:path'

// One SGR-colored ~100-char line — the balanced cat-flood shape from the drain
// investigation (docs/rust-migration/daemon-pty-drain-investigation.md).
export function floodCorpusLine(i) {
  return `\x1b[3${i % 8}mINFO\x1b[0m step ${String(i).padStart(10, '0')} lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod\n`
}

export function defaultCorpusPath(mb) {
  return join(os.tmpdir(), `orca-daemon-flood-${mb}mb.vt`)
}

// Write ≥ mb MB of flood lines to `path`; an existing file is reused as-is
// (cached across trials/runs). Resolves to the corpus byte size.
export async function ensureFloodCorpus(path, mb) {
  if (existsSync(path)) {
    return statSync(path).size
  }
  const target = mb * 1_000_000
  const out = createWriteStream(path)
  let written = 0
  let i = 0
  await new Promise((resolve, reject) => {
    out.on('error', reject)
    const writeBatch = () => {
      // Why: batch lines per write() and honor backpressure so a 500 MB corpus
      // does not balloon resident memory.
      while (written < target) {
        let batch = ''
        for (let n = 0; n < 4096 && written < target; n++) {
          const line = floodCorpusLine(i)
          batch += line
          written += line.length
          i += 1
        }
        if (!out.write(batch)) {
          out.once('drain', writeBatch)
          return
        }
      }
      out.end(resolve)
    }
    writeBatch()
  })
  return statSync(path).size
}
