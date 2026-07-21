import { readFileSync } from 'node:fs'
import { beforeAll, describe, expect, it } from 'vitest'
import { encode_key_with_mode, initSync } from './aterm_wasm.js'
import {
  ATERM_KEY_EVENT_PRESS,
  ATERM_KEY_EVENT_RELEASE,
  ATERM_KEY_EVENT_REPEAT,
  ATERM_KEY_MOD_CTRL,
  ATERM_KEY_MOD_SHIFT
} from './aterm-key-encoding'

// Kitty-protocol key-RELEASE silence, proven against the REAL committed wasm
// artifact (the other key tests mock the engine encoder; this one pins the
// binary orc actually ships). The keyup paths in aterm-textarea-input.ts
// forward every release to the engine trusting one contract: the engine
// encodes a release ONLY when the app negotiated kitty REPORT_EVENT_TYPES
// (mode bit 0x2), and NOTHING otherwise. A vendored engine that breaks it
// re-introduces the doubled-keys-in-Claude-Code bug: under Claude Code's
// `CSI > 1 u` (DISAMBIGUATE only, bit 0x1) a release came back as a CSI-u
// sequence WITHOUT the `:3` release subfield — byte-identical to a press —
// so every keystroke was delivered twice (press bytes + phantom release).

// KeyboardMode bits (aterm_types::keyboard::KeyboardMode).
const MODE_LEGACY = 0
const MODE_DISAMBIGUATE = 0x1 // what Claude Code negotiates
const MODE_DISAMBIGUATE_AND_EVENT_TYPES = 0x1 | 0x2
const MODE_REPORT_ALL_KEYS_AS_ESC = 0x100 // kitty progressive flag 8 (mode.rs 1<<8)

const ATERM_DIR = new URL('./', import.meta.url)
const decoder = new TextDecoder()

function encode(key: string, mods: number, eventType: number, modeBits: number): string {
  const bytes = encode_key_with_mode(key, mods, eventType, null, modeBits)
  return bytes ? decoder.decode(bytes) : ''
}

beforeAll(() => {
  // Real engine, loaded headlessly: initSync + on-disk bytes replaces the
  // browser fetch path (load-aterm.ts) that node tests can't use.
  initSync({ module: readFileSync(new URL('aterm_wasm_bg.wasm', ATERM_DIR)) })
})

describe('key releases are silent without kitty REPORT_EVENT_TYPES (real wasm)', () => {
  it('legacy mode: presses produce bytes, releases nothing', () => {
    expect(encode('a', 0, ATERM_KEY_EVENT_PRESS, MODE_LEGACY)).toBe('a')
    expect(encode('a', 0, ATERM_KEY_EVENT_RELEASE, MODE_LEGACY)).toBe('')
    expect(encode('Enter', 0, ATERM_KEY_EVENT_RELEASE, MODE_LEGACY)).toBe('')
  })

  it('Claude Code mode (disambiguate only): every release is silent', () => {
    // The exact doubled-keys regression: pre-fix engines returned the
    // press-identical `ESC[97u` / `ESC[13u` for these releases.
    expect(encode('a', 0, ATERM_KEY_EVENT_RELEASE, MODE_DISAMBIGUATE)).toBe('')
    expect(encode('Enter', 0, ATERM_KEY_EVENT_RELEASE, MODE_DISAMBIGUATE)).toBe('')
    expect(encode('a', ATERM_KEY_MOD_CTRL, ATERM_KEY_EVENT_RELEASE, MODE_DISAMBIGUATE)).toBe('')
  })

  it('Claude Code mode: presses and repeats stay single, press-encoded', () => {
    expect(encode('a', 0, ATERM_KEY_EVENT_PRESS, MODE_DISAMBIGUATE)).toBe('a')
    expect(encode('a', 0, ATERM_KEY_EVENT_REPEAT, MODE_DISAMBIGUATE)).toBe('a')
    expect(encode('Enter', 0, ATERM_KEY_EVENT_PRESS, MODE_DISAMBIGUATE)).toBe('\r')
    expect(encode('a', ATERM_KEY_MOD_CTRL, ATERM_KEY_EVENT_PRESS, MODE_DISAMBIGUATE)).toBe(
      '\x1b[97;5u'
    )
  })

  it('with REPORT_EVENT_TYPES negotiated, releases carry the :3 marker', () => {
    // Releases ARE reported in this mode — and must never be press-identical.
    expect(encode('a', 0, ATERM_KEY_EVENT_RELEASE, MODE_DISAMBIGUATE_AND_EVENT_TYPES)).toBe(
      '\x1b[97;1:3u'
    )
  })

  it('reset escape hatch: bare Enter/Tab/Backspace releases stay silent even with :3 reporting', () => {
    // Kitty spec (Report event types): these three keys get no release events
    // without REPORT_ALL_KEYS_AS_ESC, so `reset` still works at a stuck shell.
    expect(encode('Enter', 0, ATERM_KEY_EVENT_RELEASE, MODE_DISAMBIGUATE_AND_EVENT_TYPES)).toBe('')
    expect(encode('Tab', 0, ATERM_KEY_EVENT_RELEASE, MODE_DISAMBIGUATE_AND_EVENT_TYPES)).toBe('')
    expect(encode('Backspace', 0, ATERM_KEY_EVENT_RELEASE, MODE_DISAMBIGUATE_AND_EVENT_TYPES)).toBe(
      ''
    )
    // Chord-modified forms DO report (kitty reference impl: mods bypass the
    // silent SIMPLE() paths), keeping Shift+Tab back-tab semantics intact.
    expect(
      encode('Tab', ATERM_KEY_MOD_SHIFT, ATERM_KEY_EVENT_RELEASE, MODE_DISAMBIGUATE_AND_EVENT_TYPES)
    ).toBe('\x1b[9;2:3u')
  })
})

// Kitty-spec conformance contract of engine 3742a3b4 — the exact behaviors the
// host-side routing in this repo now RELIES on (shortcut-policy stand-downs,
// modifier-suppression gating, report-all printable routing). Each case pins
// the committed wasm so a future engine bump can't silently regress them.
describe('kitty conformance: text stays text, modifiers stay silent (real wasm)', () => {
  it('Shift-only printables stay TEXT under disambiguate (never ESC[..;2u)', () => {
    // Kitty spec: shift+letter producing text is delivered as the shifted text
    // itself. Pre-fix engines emitted the CSI-u chord ESC[97;2u for 'A'.
    expect(encode('a', ATERM_KEY_MOD_SHIFT, ATERM_KEY_EVENT_PRESS, MODE_DISAMBIGUATE)).toBe('A')
  })

  it('text presses stay text under event-type reporting', () => {
    // REPORT_EVENT_TYPES must not turn plain typing into escape reports; only
    // releases/repeats gain markers, and plain presses remain bare text.
    expect(encode('a', 0, ATERM_KEY_EVENT_PRESS, MODE_DISAMBIGUATE_AND_EVENT_TYPES)).toBe('a')
  })

  it('modifier keys are silent unless REPORT_ALL_KEYS_AS_ESC is negotiated', () => {
    // The DOM map now maps "Shift" → ShiftLeft (Left-canonical), so the gate
    // lives in the ENCODER: without kitty flag 8 a bare modifier press must
    // encode to nothing…
    expect(encode('Shift', ATERM_KEY_MOD_SHIFT, ATERM_KEY_EVENT_PRESS, MODE_DISAMBIGUATE)).toBe('')
    // …and with it, the press is reported as the kitty ShiftLeft key (57441).
    expect(
      encode('Shift', ATERM_KEY_MOD_SHIFT, ATERM_KEY_EVENT_PRESS, MODE_REPORT_ALL_KEYS_AS_ESC)
    ).toBe('\x1b[57441;2u')
  })

  it('functional-key chords encode CSI-u under disambiguate (Ctrl+Backspace)', () => {
    // The readline-compat host rewrite (Ctrl+Backspace → \x17) stands down for
    // negotiated apps because the engine emits the real chord.
    expect(encode('Backspace', ATERM_KEY_MOD_CTRL, ATERM_KEY_EVENT_PRESS, MODE_DISAMBIGUATE)).toBe(
      '\x1b[127;5u'
    )
  })
})
