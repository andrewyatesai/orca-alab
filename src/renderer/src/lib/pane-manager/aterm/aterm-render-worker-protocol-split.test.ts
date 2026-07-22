// Pins the protocol split: the wire contract grew to 297/300 counted lines against
// the oxlint max-lines cap (no disable/baseline bump permitted), so the worker → main
// half moved to aterm-worker-event-protocol. These tests keep an early-warning buffer
// under the cap and prove the entry point still re-exports the whole event contract.
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import type {
  AtermWorkerBooted,
  AtermWorkerCrash,
  AtermWorkerMessage,
  AtermWorkerPaneCommand,
  AtermWorkerPaneEvent,
  AtermWorkerRequest,
  AtermWorkerSearchNext,
  AtermWorkerState
} from './aterm-render-worker-protocol'

/** Mirrors oxlint max-lines with skipBlankLines + skipComments (comment-only lines). */
function countedLines(source: string): number {
  let count = 0
  let inBlockComment = false
  for (const raw of source.split('\n')) {
    const line = raw.trim()
    if (inBlockComment) {
      if (line.includes('*/')) {
        inBlockComment = false
      }
      continue
    }
    if (line === '' || line.startsWith('//')) {
      continue
    }
    if (line.startsWith('/*')) {
      if (!line.includes('*/')) {
        inBlockComment = true
      }
      continue
    }
    count++
  }
  return count
}

// Early-warning buffer: the oxlint cap is 300 counted lines and AGENTS.md forbids any
// max-lines disable or baseline bump — fail HERE first, while a further split is cheap.
const EARLY_WARNING_MAX = 280

describe('aterm render-worker protocol split', () => {
  it.each([['aterm-render-worker-protocol.ts'], ['aterm-worker-event-protocol.ts']])(
    '%s keeps headroom under the oxlint max-lines cap',
    (file) => {
      const source = readFileSync(fileURLToPath(new URL(`./${file}`, import.meta.url)), 'utf8')
      expect(countedLines(source)).toBeLessThanOrEqual(EARLY_WARNING_MAX)
    }
  )

  it('the protocol entry point still exposes both halves of the wire contract', () => {
    // Compile-time: the moved event types must remain importable from the original
    // entry point and participate in the wire unions exactly as before the split.
    type Extends<A, B> = [A] extends [B] ? true : false
    const stateIsPaneEvent: Extends<AtermWorkerState, AtermWorkerPaneEvent> = true
    const paneEventRidesWire: Extends<
      AtermWorkerPaneEvent & { paneId: number },
      AtermWorkerMessage
    > = true
    const bootedRidesWire: Extends<AtermWorkerBooted, AtermWorkerMessage> = true
    const crashRidesWire: Extends<AtermWorkerCrash, AtermWorkerMessage> = true
    const commandRidesRequest: Extends<
      AtermWorkerPaneCommand & { paneId: number },
      AtermWorkerRequest
    > = true
    const searchStaysCommand: Extends<AtermWorkerSearchNext, AtermWorkerPaneCommand> = true
    expect([
      stateIsPaneEvent,
      paneEventRidesWire,
      bootedRidesWire,
      crashRidesWire,
      commandRidesRequest,
      searchStaysCommand
    ]).toEqual([true, true, true, true, true, true])
  })
})
