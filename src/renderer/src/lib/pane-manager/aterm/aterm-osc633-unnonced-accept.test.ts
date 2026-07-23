import { readFileSync } from 'node:fs'
import { beforeAll, describe, expect, it } from 'vitest'
import { initSync, AtermTerminal } from './aterm_wasm.js'
import { ATERM_RENDERER_FONT_PX } from './aterm-pane-controller-types'

// Un-nonced OSC 633;E acceptance, pinned against the REAL committed wasm
// artifact (#7596 Critic note). The engine's OSC 133/633 shell-mark parsing is
// nonce-gated ONLY when a capability nonce has been set
// (handler_osc_shell.rs), and this fork sets none — Orca's own shell hooks
// emit bare `ESC ] 633;E;<command> BEL`. If a future engine pin adopts nonce
// enforcement by default, these assertions fail loudly instead of the
// command-line feature dying silently.

const ATERM_DIR = new URL('./', import.meta.url)
const FONT_URL = new URL('../../../assets/fonts/jetbrains-mono.ttf', import.meta.url)

const ROWS = 10
const COLS = 60

let fontBytes: Uint8Array

beforeAll(() => {
  // Real engine, loaded headlessly: initSync + on-disk bytes replaces the
  // browser fetch path (load-aterm.ts) that node tests can't use.
  initSync({ module: readFileSync(new URL('aterm_wasm_bg.wasm', ATERM_DIR)) })
  fontBytes = new Uint8Array(readFileSync(FONT_URL))
})

function openTerminal(): AtermTerminal {
  return new AtermTerminal(
    ROWS,
    COLS,
    fontBytes,
    ATERM_RENDERER_FONT_PX,
    0xffffff,
    0x000000,
    0xffffff,
    0x334455
  )
}

function gridText(term: AtermTerminal): string {
  let text = ''
  for (let row = 0; row < ROWS; row += 1) {
    text += `${term.row_text(row) ?? ''}\n`
  }
  return text
}

describe('un-nonced OSC 633;E is accepted by the engine (real wasm)', () => {
  it('consumes bare 633;E as an OSC without leaking payload into the grid', () => {
    const term = openTerminal()
    try {
      term.process_str('$ \x1b]633;E;npm run dev\x07')
      term.process_str('\x1b]133;C\x07dev output line\r\n')

      const grid = gridText(term)
      // The sequence was consumed whole: neither the command payload nor a
      // half-parsed `]633` fragment rendered as text.
      expect(grid).not.toContain('npm run dev')
      expect(grid).not.toContain('633')
      expect(grid).toContain('$')
      expect(grid).toContain('dev output line')
    } finally {
      term.free()
    }
  })

  it('a 633;E in the stream leaves the un-nonced 133 shell-mark lifecycle intact', () => {
    const term = openTerminal()
    try {
      term.process_str('\x1b]133;A\x07$ ')
      term.process_str('\x1b]633;E;npm run dev\x07')
      term.process_str('\x1b]133;C\x07dev output line\r\n')
      term.process_str('\x1b]133;D;0\x07\x1b]133;A\x07$ ')

      // The 133 marks ride the same nonce gate as 633 (handler_osc_shell.rs);
      // their un-nonced acceptance is the engine's observable for the shared
      // parse path at this pin (no wasm command-line accessor exists yet).
      const events = JSON.parse(term.take_osc_events() ?? '[]') as [number, string][]
      const shellMarks = events.filter(([code]) => code === 133).map(([, payload]) => payload[0])
      expect(shellMarks).toEqual(['A', 'C', 'D', 'A'])
    } finally {
      term.free()
    }
  })

  it('an un-nonced 633 lifecycle builds the block ledger (E rides the same gate)', () => {
    const term = openTerminal()
    try {
      // A 633-only lifecycle: prompt → command input → explicit command text
      // (E) → exec → output → finished; the next prompt's A seals the block.
      term.process_str('\x1b]633;A\x07$ npm run dev')
      term.process_str('\x1b]633;B\x07\r\n')
      term.process_str('\x1b]633;E;npm run dev\x07')
      term.process_str('\x1b]633;C\x07dev output line\r\n')
      term.process_str('\x1b]633;D;0\x07')
      term.process_str('\x1b]633;A\x07$ ')

      // A sealed, readable block is possible ONLY if the bare (un-nonced) 633
      // sequences passed shell_nonce_gate_ok — handler_osc_633 gates ONCE at
      // entry, so the same acceptance admits 633;E's commandline (stored on
      // this block; preferred by block_command_text over screen extraction).
      // A nonce-enforcing future pin would drop every mark and this read
      // would return undefined — failing loudly, per the Critic note.
      const raw = term.last_command_output()
      expect(raw, 'un-nonced 633 lifecycle must yield a completed block').toBeTruthy()
      const parsed = JSON.parse(raw as string) as {
        status: string
        text?: string
        exitCode?: number | null
      }
      expect(parsed.status).toBe('ok')
      expect(parsed.text).toContain('dev output line')
      expect(parsed.exitCode).toBe(0)
    } finally {
      term.free()
    }
  })
})
