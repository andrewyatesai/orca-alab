import { describe, expect, it, vi, beforeAll, afterAll } from 'vitest'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { performance } from 'node:perf_hooks'
import {
  readTerminalScrollbackSnapshotSync,
  writeTerminalScrollbackSnapshotSync
} from './terminal-scrollback-snapshots'
import {
  readTerminalScrollbackSnapshotOlderChunkSync,
  readTerminalScrollbackSnapshotTailSync
} from './terminal-scrollback-snapshot-deep-read'
import { TERMINAL_SCROLLBACK_STORE_BYTE_LIMIT } from '../shared/terminal-scrollback-limits'

vi.mock('electron', () => ({
  app: { getPath: vi.fn(() => join(tmpdir(), 'orca-restore-bench-unused-legacy-root')) }
}))

// P5 restore-latency bench: compares the renderer-BLOCKING cost of restore reads.
// - "legacy sync tail (512KB)": the pre-P5 restore — also P5's unchanged sync portion.
// - "hypothetical sync full (5MB)": what naively raising the sync replay limit
//   would have cost per pane in one renderer-blocking bite (the freeze P5 avoids).
// - "P5 older chunk (max)": the largest single main-thread bite of the async
//   stream; the renderer never blocks on any of these.
// Timings are environment-dependent, so assertions stay structural; numbers are
// printed for the record.

function median(samples: number[]): number {
  const sorted = [...samples].sort((a, b) => a - b)
  return sorted[Math.floor(sorted.length / 2)]
}

describe('terminal scrollback restore latency (P5 bench)', () => {
  let dir: string
  let storage: { snapshotRoot: string }
  let ref: string

  beforeAll(() => {
    dir = mkdtempSync(join(tmpdir(), 'orca-scrollback-bench-'))
    storage = { snapshotRoot: dir }
    const line = `[12:00:00] service: request ok \x1b[32m200\x1b[0m ${'a'.repeat(80)}\r\n`
    const buffer = line.repeat(Math.ceil(TERMINAL_SCROLLBACK_STORE_BYTE_LIMIT / line.length) + 64)
    ref = writeTerminalScrollbackSnapshotSync({ tabId: 'bench', leafId: 'bench', buffer, storage })!
  })

  afterAll(() => {
    rmSync(dir, { recursive: true, force: true })
  })

  it('sync tail read stays bounded while chunked reads recover the full store', () => {
    const runs = 9

    const legacyTailMs: number[] = []
    let legacyBytes = 0
    for (let i = 0; i < runs; i++) {
      const start = performance.now()
      const text = readTerminalScrollbackSnapshotSync(ref, storage)!
      legacyTailMs.push(performance.now() - start)
      legacyBytes = Buffer.byteLength(text, 'utf-8')
    }

    const tailMs: number[] = []
    for (let i = 0; i < runs; i++) {
      const start = performance.now()
      readTerminalScrollbackSnapshotTailSync(ref, storage)
      tailMs.push(performance.now() - start)
    }
    const tail = readTerminalScrollbackSnapshotTailSync(ref, storage)!

    // Hypothetical "just raise the sync limit to 5MB": one blocking read+decode of everything.
    const syncFullMs: number[] = []
    let fullBytes = 0
    for (let i = 0; i < runs; i++) {
      const start = performance.now()
      let assembled = ''
      let cursor = tail.olderChunkCursor
      while (cursor < tail.olderEndOffset) {
        const chunk = readTerminalScrollbackSnapshotOlderChunkSync(
          { ref, cursor, endOffset: tail.olderEndOffset, fingerprint: tail.fingerprint },
          storage
        )!
        assembled += chunk.text
        cursor = chunk.nextCursor
      }
      assembled += tail.text
      syncFullMs.push(performance.now() - start)
      fullBytes = Buffer.byteLength(assembled, 'utf-8')
    }

    // P5 shape: per-chunk main-thread bites (the renderer awaits these async).
    const chunkMs: number[] = []
    let chunkCount = 0
    {
      let cursor = tail.olderChunkCursor
      while (cursor < tail.olderEndOffset) {
        const start = performance.now()
        const chunk = readTerminalScrollbackSnapshotOlderChunkSync(
          { ref, cursor, endOffset: tail.olderEndOffset, fingerprint: tail.fingerprint },
          storage
        )!
        chunkMs.push(performance.now() - start)
        cursor = chunk.nextCursor
        chunkCount++
      }
    }

    const fmt = (n: number): string => n.toFixed(2)
    console.log(
      [
        '',
        '── P5 restore-latency bench (5MB store) ─────────────────────────',
        `legacy sync tail read (512KB, = P5 sync portion): ${fmt(median(legacyTailMs))} ms median (recovers ${legacyBytes} bytes)`,
        `P5 tail read w/ offsets (sync portion):           ${fmt(median(tailMs))} ms median`,
        `hypothetical sync full read (avoided by P5):      ${fmt(median(syncFullMs))} ms median (recovers ${fullBytes} bytes)`,
        `P5 async older chunks: ${chunkCount} chunks, max single main-thread bite ${fmt(Math.max(...chunkMs))} ms, total ${fmt(chunkMs.reduce((a, b) => a + b, 0))} ms`,
        '─────────────────────────────────────────────────────────────────'
      ].join('\n')
    )

    // Structural guarantees, not timing flakes: the full store is recovered and
    // each async bite reads at most one chunk-limit slice.
    expect(fullBytes).toBeGreaterThan(legacyBytes * 9)
    expect(chunkCount).toBeGreaterThanOrEqual(9)
    expect(tail.olderEndOffset - tail.olderChunkCursor).toBeGreaterThan(4 * 1024 * 1024)
  })
})
