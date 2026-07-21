import type { TerminalModes } from './types'

// DECSET params for each mouse reporting protocol the scanner can track.
const MOUSE_PROTOCOL_MODE_PARAMS = [
  ['x10', '9'],
  ['vt200', '1000'],
  ['drag', '1002'],
  ['any', '1003']
] as const

// Why no kitty flags here: rehydrateSequences feeds renderer xterms, and
// POST_REPLAY_REATTACH_RESET's deliberate kitty reset (stale CSI-u Ctrl+C
// hazard) must stay authoritative. modes.kittyKeyboardFlags exists for
// emulator re-seed parity only; a re-seeded emulator answers ?0u and
// protocol-conformant programs re-push.
export function buildRehydrateSequences(modes: TerminalModes): string {
  const seqs: string[] = []
  if (modes.alternateScreen) {
    // Why: normal-buffer serialization can leave its pen active, while the
    // separately serialized alt body assumes it starts from default SGR.
    seqs.push('\x1b[0m\x1b[?1049h')
  }
  if (modes.bracketedPaste) {
    seqs.push('\x1b[?2004h')
  }
  if (modes.applicationCursor) {
    seqs.push('\x1b[?1h')
  }
  // Why explicit h AND l for every mouse mode (#8335): these sequences also
  // replay into LIVE renderer engines (hidden-pane restore / warm reattach),
  // where arm-only output left a stale renderer-side mouse arm after the app
  // disarmed while the pane was hidden — the revealed pane then echoed motion
  // reports (`<35;x;yM`) as literal input. Disarms first and the active arm
  // last, so every restore ends on the daemon engine's authoritative state.
  // (Arming itself is also load-bearing: mobile alt-screen scroll gestures
  // need the mouse mode restored from cold snapshots; OpenCode/OpenTUI
  // enables scrollable panes this way.)
  const activeMouseProtocol = modes.mouseTracking ? (modes.mouseTrackingMode ?? 'vt200') : 'none'
  // Note: xterm tracks the reporting protocol and the SGR encodings as
  // independent modes, so the encodings are resynced explicitly even when
  // reporting is off. All disarms precede all arms so an engine that treats
  // any protocol DECRST as a full disable still ends on the armed state.
  for (const [protocol, param] of MOUSE_PROTOCOL_MODE_PARAMS) {
    if (protocol !== activeMouseProtocol) {
      seqs.push(`\x1b[?${param}l`)
    }
  }
  if (!modes.sgrMouseMode) {
    seqs.push('\x1b[?1006l')
  }
  if (!modes.sgrMousePixelsMode) {
    seqs.push('\x1b[?1016l')
  }
  for (const [protocol, param] of MOUSE_PROTOCOL_MODE_PARAMS) {
    if (protocol === activeMouseProtocol) {
      seqs.push(`\x1b[?${param}h`)
    }
  }
  if (modes.sgrMouseMode) {
    seqs.push('\x1b[?1006h')
  }
  if (modes.sgrMousePixelsMode) {
    seqs.push('\x1b[?1016h')
  }
  return seqs.join('')
}
