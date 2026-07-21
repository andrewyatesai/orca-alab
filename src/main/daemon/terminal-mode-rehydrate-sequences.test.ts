import { describe, expect, it } from 'vitest'
import { buildRehydrateSequences } from './terminal-mode-rehydrate-sequences'
import type { TerminalModes } from './types'

function makeModes(overrides: Partial<TerminalModes> = {}): TerminalModes {
  return {
    bracketedPaste: false,
    mouseTracking: false,
    applicationCursor: false,
    alternateScreen: false,
    ...overrides
  }
}

describe('buildRehydrateSequences mouse-mode resync (#8335)', () => {
  it('explicitly disarms every mouse mode when the daemon engine says tracking is off', () => {
    // Why: rehydrate replays into LIVE renderer engines on hidden-pane restore.
    // A pane hidden while the app disarmed mouse tracking (Claude Code handing
    // the foreground to an external editor) kept a stale renderer-side arm and
    // echoed `<35;x;yM` motion reports as literal input on reveal.
    const seqs = buildRehydrateSequences(makeModes())
    expect(seqs).toContain('\x1b[?9l')
    expect(seqs).toContain('\x1b[?1000l')
    expect(seqs).toContain('\x1b[?1002l')
    expect(seqs).toContain('\x1b[?1003l')
    expect(seqs).toContain('\x1b[?1006l')
    expect(seqs).toContain('\x1b[?1016l')
    expect(seqs).not.toContain('h')
  })

  it('arms the active protocol AFTER disarming the inactive ones', () => {
    const seqs = buildRehydrateSequences(
      makeModes({ mouseTracking: true, mouseTrackingMode: 'any', sgrMouseMode: true })
    )
    // The arm must come last so an engine that treats any protocol DECRST as a
    // full disable still ends armed on the daemon's authoritative protocol.
    expect(seqs.indexOf('\x1b[?1003h')).toBeGreaterThan(seqs.lastIndexOf('l'))
    expect(seqs).toContain('\x1b[?9l')
    expect(seqs).toContain('\x1b[?1000l')
    expect(seqs).toContain('\x1b[?1002l')
    expect(seqs).not.toContain('\x1b[?1003l')
    expect(seqs).toContain('\x1b[?1006h')
    expect(seqs).not.toContain('\x1b[?1006l')
    expect(seqs).toContain('\x1b[?1016l')
  })

  it('defaults an armed tracker without a recorded protocol to vt200', () => {
    const seqs = buildRehydrateSequences(makeModes({ mouseTracking: true }))
    expect(seqs).toContain('\x1b[?1000h')
    expect(seqs).not.toContain('\x1b[?1000l')
  })

  it('resyncs SGR pixel encoding independently of the reporting protocol', () => {
    const seqs = buildRehydrateSequences(
      makeModes({ mouseTracking: true, mouseTrackingMode: 'drag', sgrMousePixelsMode: true })
    )
    expect(seqs).toContain('\x1b[?1002h')
    expect(seqs).toContain('\x1b[?1016h')
    expect(seqs).not.toContain('\x1b[?1016l')
    expect(seqs).toContain('\x1b[?1006l')
  })

  it('keeps non-mouse modes arm-only', () => {
    const armed = buildRehydrateSequences(
      makeModes({ alternateScreen: true, bracketedPaste: true, applicationCursor: true })
    )
    expect(armed).toContain('\x1b[0m\x1b[?1049h')
    expect(armed).toContain('\x1b[?2004h')
    expect(armed).toContain('\x1b[?1h')
    const idle = buildRehydrateSequences(makeModes())
    expect(idle).not.toContain('\x1b[?1049l')
    expect(idle).not.toContain('\x1b[?2004l')
    expect(idle).not.toContain('\x1b[?1l')
  })

  it('never pushes kitty keyboard flags', () => {
    const seqs = buildRehydrateSequences(makeModes({ kittyKeyboardFlags: 0b101 }))
    expect(seqs).not.toContain('u')
  })
})
