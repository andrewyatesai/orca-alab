// Fed §2.4 (remote wire): host-side ledger of snapshot row anchors.
//
// Every combined wire snapshot the host serializes gets a fresh `anchorGen`
// and records `hostRowAnchor` — the STABLE host row (retained-origin-based,
// eviction-proof) of the FIRST row serialized into that snapshot. The
// snapshot reply carries both; a later `terminal.search` echoes them back
// ONLY for the generation the client says it replayed, and only while the
// emulator incarnation that minted them is still alive — so a client never
// remaps match rows against a snapshot it didn't replay, or across an
// emulator rebuild that restarted the coordinate space (the critic's
// wrong-row-jump hole).

export type HostRowAnchorRecord = {
  hostRowAnchor: number
  anchorGen: number
  /** Emulator lifetime that minted this anchor (stable rows reset with it). */
  incarnation: number
  /** Host grid width at serialization — wrap-width approximation signal. */
  hostCols: number
}

// Why bounded: a client holds at most one replayed snapshot, but resyncs can
// race a search; a small ring keeps the last few generations addressable
// without unbounded per-PTY growth.
const MAX_RETAINED_GENS_PER_PTY = 8

export class TerminalHostRowAnchorLedger {
  private nextGen = 1
  private readonly byPty = new Map<string, HostRowAnchorRecord[]>()

  /** Record the anchor for a freshly serialized snapshot; returns its gen. */
  mint(
    ptyId: string,
    anchor: { hostRowAnchor: number; incarnation: number; hostCols: number }
  ): number {
    const anchorGen = this.nextGen++
    const records = this.byPty.get(ptyId) ?? []
    records.push({ ...anchor, anchorGen })
    if (records.length > MAX_RETAINED_GENS_PER_PTY) {
      records.splice(0, records.length - MAX_RETAINED_GENS_PER_PTY)
    }
    this.byPty.set(ptyId, records)
    return anchorGen
  }

  /** The anchor a search response may echo: exact (ptyId, anchorGen) match
   *  whose minting incarnation equals the CURRENT one, else null (client
   *  degrades to inline context expansion instead of a wrong-row jump). */
  lookup(ptyId: string, anchorGen: number, currentIncarnation: number | null): HostRowAnchorRecord | null {
    if (currentIncarnation === null) {
      return null
    }
    const record = this.byPty.get(ptyId)?.find((entry) => entry.anchorGen === anchorGen)
    return record && record.incarnation === currentIncarnation ? record : null
  }

  /** Drop a PTY's anchors (close/teardown). */
  clearPty(ptyId: string): void {
    this.byPty.delete(ptyId)
  }
}

// Why module state (pattern of terminal-model-query-authority.ts): the
// serialize helpers in rpc/methods/terminal.ts and the search handler must
// share one ledger without threading it through the runtime service.
let ledger = new TerminalHostRowAnchorLedger()

export function terminalHostRowAnchorLedger(): TerminalHostRowAnchorLedger {
  return ledger
}

export function resetTerminalHostRowAnchorLedgerForTest(): void {
  ledger = new TerminalHostRowAnchorLedger()
}
