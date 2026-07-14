import { describe, expect, it } from 'vitest'
import {
  POST_REPLAY_LIVE_AGENT_REATTACH_RESET,
  POST_REPLAY_LIVE_SNAPSHOT_RESET,
  POST_REPLAY_MODE_RESET,
  POST_REPLAY_REATTACH_RESET,
  RESET_KITTY_KEYBOARD_PROTOCOL,
  RESET_MOUSE_REPORTING,
  RESET_TERMINAL_CURSOR_STYLE
} from './layout-serialization'

// The original suite wrote these reset bundles to a headless xterm and read its
// private `_core.coreService` (decPrivateModes / kittyKeyboard) to prove the
// EFFECT on a live VT engine. Under aterm there is no in-process engine in
// vitest, and inspecting another emulator's internals was an xterm-coupled
// characterization with no public aterm equivalent. The reset-sequence effect on
// the live engine is now an aterm concern, covered by the cursor/restore e2e
// specs (tests/e2e/terminal-cursor-inactive-style.spec.ts,
// tests/e2e/terminal-tab-switch-visual-restore.spec.ts).
//
// What still belongs renderer-side is the literal byte content of these reset
// constants — a bad reset string is the regression these guards exist for — so
// assert it directly here (pure string checks, no VT engine).
describe('terminal replay state reset constants', () => {
  it('encodes the DECSCUSR cursor-style + Kitty keyboard reset primitives', () => {
    // DECSCUSR `0 q`: reset cursor style/blink to the user's configured default.
    expect(RESET_TERMINAL_CURSOR_STYLE).toBe('\x1b[0 q')
    // Kitty keyboard pop-to-empty (`<99u`) then flags-clear (`=0u`).
    expect(RESET_KITTY_KEYBOARD_PROTOCOL).toBe('\x1b[<99u\x1b[=0u')
  })

  it('clears mouse / focus / bracketed-paste modes in the cold-restore bundle', () => {
    // Cold restore lands a fresh shell, so every interactive mode bit is reset:
    // cursor style + Kitty + DECTCEM `?25h` + the full mouse set (#7893) + focus
    // 1004 + bracketed-paste 2004.
    expect(POST_REPLAY_MODE_RESET).toBe(
      `${RESET_TERMINAL_CURSOR_STYLE}${RESET_KITTY_KEYBOARD_PROTOCOL}\x1b[?25h${RESET_MOUSE_REPORTING}\x1b[?1004l\x1b[?2004l`
    )
    expect(POST_REPLAY_MODE_RESET).toContain(RESET_KITTY_KEYBOARD_PROTOCOL)
  })

  it('clears leaked mouse modes on reattach (#7893), keeping bracketed paste', () => {
    // A snapshot's rehydrate re-arms whatever mouse mode a possibly-uncleanly
    // killed TUI left on, so a plain shell would echo motion reports as literal
    // input; reattach clears the full mouse set for that reason (#7893). Live
    // agents keep mouse via POST_REPLAY_LIVE_AGENT_REATTACH_RESET.
    expect(POST_REPLAY_REATTACH_RESET).toBe(
      `${RESET_TERMINAL_CURSOR_STYLE}${RESET_KITTY_KEYBOARD_PROTOCOL}\x1b[?25h${RESET_MOUSE_REPORTING}\x1b[?1004l`
    )
    expect(POST_REPLAY_REATTACH_RESET).toContain(RESET_KITTY_KEYBOARD_PROTOCOL)
    expect(POST_REPLAY_REATTACH_RESET).toContain('\x1b[?1000l')
    // Bracketed paste (2004) stays armed on reattach — only the cold restore clears it.
    expect(POST_REPLAY_REATTACH_RESET).not.toContain('\x1b[?2004l')
  })

  it('keeps focus reporting on the live-agent reattach reset (upstream #7061)', () => {
    // A live agent that draws while parked relies on focus-in/out: the agent
    // variant resets cursor style + Kitty + DECTCEM but must NOT clear 1004.
    expect(POST_REPLAY_LIVE_AGENT_REATTACH_RESET).toBe(
      `${RESET_TERMINAL_CURSOR_STYLE}${RESET_KITTY_KEYBOARD_PROTOCOL}\x1b[?25h`
    )
    expect(POST_REPLAY_LIVE_AGENT_REATTACH_RESET).not.toContain('\x1b[?1004l')
  })

  it('preserves Kitty keyboard flags on the live-output snapshot reset', () => {
    // Hidden-output recovery replays the SAME live session, so the still-running
    // foreground TUI's Kitty keyboard flags must survive — only cursor style,
    // DECTCEM and focus reporting are reset.
    expect(POST_REPLAY_LIVE_SNAPSHOT_RESET).toBe(
      `${RESET_TERMINAL_CURSOR_STYLE}\x1b[?25h\x1b[?1004l`
    )
    expect(POST_REPLAY_LIVE_SNAPSHOT_RESET).not.toContain(RESET_KITTY_KEYBOARD_PROTOCOL)
  })
})
