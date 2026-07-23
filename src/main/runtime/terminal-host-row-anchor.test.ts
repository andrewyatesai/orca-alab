import { beforeEach, describe, expect, it } from 'vitest'
import {
  resetTerminalHostRowAnchorLedgerForTest,
  terminalHostRowAnchorLedger
} from './terminal-host-row-anchor'

describe('TerminalHostRowAnchorLedger', () => {
  beforeEach(() => {
    resetTerminalHostRowAnchorLedgerForTest()
  })

  it('mints monotonically increasing gens and looks them up per pty', () => {
    const ledger = terminalHostRowAnchorLedger()
    const genA = ledger.mint('pty-1', { hostRowAnchor: 100, incarnation: 7, hostCols: 80 })
    const genB = ledger.mint('pty-1', { hostRowAnchor: 140, incarnation: 7, hostCols: 80 })
    expect(genB).toBeGreaterThan(genA)
    expect(ledger.lookup('pty-1', genA, 7)).toMatchObject({ hostRowAnchor: 100, anchorGen: genA })
    expect(ledger.lookup('pty-1', genB, 7)).toMatchObject({ hostRowAnchor: 140, anchorGen: genB })
  })

  it('refuses lookups across pty ids, unknown gens, and emulator incarnations', () => {
    const ledger = terminalHostRowAnchorLedger()
    const gen = ledger.mint('pty-1', { hostRowAnchor: 100, incarnation: 7, hostCols: 80 })
    expect(ledger.lookup('pty-2', gen, 7)).toBeNull()
    expect(ledger.lookup('pty-1', gen + 999, 7)).toBeNull()
    // A rebuilt emulator restarted stable-row coordinates: the old anchor must
    // never validate against the new space (wrong-row-jump hole).
    expect(ledger.lookup('pty-1', gen, 8)).toBeNull()
    expect(ledger.lookup('pty-1', gen, null)).toBeNull()
  })

  it('retains only a bounded number of generations per pty', () => {
    const ledger = terminalHostRowAnchorLedger()
    const gens = Array.from({ length: 12 }, (_, i) =>
      ledger.mint('pty-1', { hostRowAnchor: i, incarnation: 1, hostCols: 80 })
    )
    expect(ledger.lookup('pty-1', gens.at(0)!, 1)).toBeNull()
    expect(ledger.lookup('pty-1', gens.at(-1)!, 1)).not.toBeNull()
  })

  it('clearPty drops all anchors for that pty only', () => {
    const ledger = terminalHostRowAnchorLedger()
    const genA = ledger.mint('pty-1', { hostRowAnchor: 1, incarnation: 1, hostCols: 80 })
    const genB = ledger.mint('pty-2', { hostRowAnchor: 2, incarnation: 1, hostCols: 80 })
    ledger.clearPty('pty-1')
    expect(ledger.lookup('pty-1', genA, 1)).toBeNull()
    expect(ledger.lookup('pty-2', genB, 1)).not.toBeNull()
  })
})
