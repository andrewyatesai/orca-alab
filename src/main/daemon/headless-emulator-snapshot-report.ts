// Snapshot report assembly over the raw engine handle, split from
// headless-emulator.ts (line budget). Builds the split snapshotAnsi /
// scrollbackAnsi shape (with the poisoned-engine degraded fallback) and the
// OSC-8 link merge; the emulator supplies its panic-containment runner.
import { mergeRestoredOscLinks } from './terminal-osc-link-merge'
import { buildRehydrateSequences } from './terminal-mode-rehydrate-sequences'
import type { RustHeadlessTerminalHandle } from './rust-terminal-addon'
import type { TerminalSnapshot, TerminalModes } from './types'
import type { TerminalOscLinkRange } from '../../shared/terminal-osc-link-ranges'

export type EmulatorSnapshotContext = {
  /** The emulator's engineCall: poison-containing runner for native calls. */
  run<T>(op: string, call: () => T, fallback: () => T): T
  term: RustHeadlessTerminalHandle
  modes: TerminalModes
  scrollbackRows?: number
  cols: number
  rows: number
  cwd: string | null
  lastTitle?: string
  /** Why: written LAST by the restorer (after any reset) so the next live
   *  chunk completes this dangling sequence instead of rendering it literally
   *  (Bug E / #7329). Its bytes are already counted by the snapshot seq. */
  partialEscapeTail: string
  restoredOscLinks: TerminalOscLinkRange[]
}

/** Live links come from aterm — both the visible grid and scrollback history
 *  (aterm retains hyperlink spans on scroll) — merged with checkpoint-restored
 *  links so a restored buffer keeps clickable links. */
export function collectEmulatorOscLinks(
  ctx: Pick<EmulatorSnapshotContext, 'term' | 'restoredOscLinks' | 'cols'>,
  scrollbackRows?: number
): TerminalOscLinkRange[] {
  return mergeRestoredOscLinks(
    ctx.term.oscLinkRanges(scrollbackRows),
    ctx.restoredOscLinks,
    ctx.cols
  )
}

export function buildEmulatorSnapshotReport(ctx: EmulatorSnapshotContext): TerminalSnapshot {
  const pendingEscapeTail =
    ctx.partialEscapeTail.length > 0 ? { pendingEscapeTailAnsi: ctx.partialEscapeTail } : {}
  const shared = {
    rehydrateSequences: buildRehydrateSequences(ctx.modes),
    cwd: ctx.cwd,
    modes: ctx.modes,
    cols: ctx.cols,
    rows: ctx.rows,
    lastTitle: ctx.lastTitle,
    ...pendingEscapeTail
  }
  return ctx.run(
    'serialize',
    () => ({
      snapshotAnsi: ctx.term.serializeAnsi(ctx.scrollbackRows),
      // SPLIT shape: the visible viewport lives in snapshotAnsi, history in
      // scrollbackAnsi (independent of scrollbackRows). The alt-screen
      // cold-restore path needs this — the alt buffer has no scrollback, so its
      // pre-TUI history is only recoverable here.
      scrollbackAnsi: ctx.term.serializeScrollbackAnsi(),
      oscLinks: collectEmulatorOscLinks(ctx, ctx.scrollbackRows),
      scrollbackLines: ctx.term.scrollbackLen(),
      ...shared
    }),
    // Poisoned engine: no replayable buffer to offer, but the scanned state
    // (cwd/modes/title/partial-tail) is still honest, so reconnect/rehydrate
    // keep working.
    () => ({
      snapshotAnsi: '',
      scrollbackAnsi: '',
      oscLinks: ctx.restoredOscLinks.slice(),
      scrollbackLines: 0,
      ...shared
    })
  )
}
