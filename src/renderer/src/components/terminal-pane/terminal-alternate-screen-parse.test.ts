import { readFileSync } from 'node:fs'
import { afterEach, beforeAll, describe, expect, it } from 'vitest'
import { initSync, AtermTerminal } from '@/lib/pane-manager/aterm/aterm_wasm.js'
import { buildAtermEngineReads } from '@/lib/pane-manager/aterm/aterm-engine-reads'
import {
  createAtermFacadeBuffer,
  type AtermBufferSource
} from '@/lib/pane-manager/aterm/aterm-facade-buffer'
import { ATERM_RENDERER_FONT_PX } from '@/lib/pane-manager/aterm/aterm-pane-controller-types'

const COLS = 120
const ROWS = 34

const ATERM_DIR = new URL('../../lib/pane-manager/aterm/', import.meta.url)
const FONT_URL = new URL('../../assets/fonts/jetbrains-mono.ttf', import.meta.url)

let fontBytes: Uint8Array

beforeAll(() => {
  // Real engine, loaded headlessly: initSync + on-disk bytes replaces the
  // browser fetch path (load-aterm.ts) that node tests can't use.
  initSync({ module: readFileSync(new URL('aterm_wasm_bg.wasm', ATERM_DIR)) })
  fontBytes = new Uint8Array(readFileSync(FONT_URL))
})

type ParsedTerminalHarness = {
  /** One PTY output chunk: engine process() + the facade's post-process
   *  buffer-change poll — the same pair feedEngine/drainEngineSideChannels
   *  runs for every chunk in aterm-terminal-facade. */
  feedChunk: (data: string) => void
  /** The exact read pty-connection's atlas recovery makes at parse time. */
  bufferType: () => 'normal' | 'alternate'
  /** onBufferChange events observed since open (the runtime's switch counter). */
  switches: () => number
  dispose: () => void
}

// Wires the REAL production seam end to end: wasm parser → buildAtermEngineReads
// → createAtermFacadeBuffer. Only gridSize is test-supplied (production sources
// it from the grid-sizing module, not the reads bundle).
function openParsedTerminal(): ParsedTerminalHarness {
  const term = new AtermTerminal(
    ROWS,
    COLS,
    fontBytes,
    ATERM_RENDERER_FONT_PX,
    0xffffff,
    0x000000,
    0xffffff,
    0x334455
  )
  const reads = buildAtermEngineReads(
    term,
    { dpr: 1, cellWidth: 0, cellHeight: 0 },
    () => undefined,
    () => false
  )
  const source: AtermBufferSource = { ...reads, gridSize: () => ({ cols: COLS, rows: ROWS }) }
  const { buffer, pollBufferChange } = createAtermFacadeBuffer(() => source)
  // Attach-time drain (facade __attachController): seeds the poll's last-seen
  // type so the first PTY chunk reports only a real flip, not the baseline.
  pollBufferChange()
  let switches = 0
  buffer.onBufferChange(() => {
    switches += 1
  })
  return {
    feedChunk: (data: string) => {
      term.process_str(data)
      pollBufferChange()
    },
    bufferType: () => buffer.active.type,
    switches: () => switches,
    dispose: () => term.free()
  }
}

// Pins the aterm contract the alternate-screen atlas recovery relies on
// (pty-connection's alternateScreenRewriteAtlasRecoveryOnParsed): the engine
// parses each chunk synchronously, so by the time the facade fires a write
// callback, buffer.active.type reflects any alternate-screen enter/exit in
// that chunk — even when the DECSET sequence splits across PTY chunk
// boundaries. Fork port of the upstream @xterm/headless contract pin.
describe('alternate-screen buffer state at parse time', () => {
  let harness: ParsedTerminalHarness | undefined

  afterEach(() => {
    harness?.dispose()
    harness = undefined
  })

  it('reflects an enter sequence split across two chunks', () => {
    harness = openParsedTerminal()
    harness.feedChunk('\x1b[?104')
    expect(harness.bufferType()).toBe('normal')
    harness.feedChunk('9h\x1b[2J\x1b[H~\x1b[K')
    expect(harness.bufferType()).toBe('alternate')
    expect(harness.switches()).toBe(1)
  })

  it('fires onBufferChange once per chunk-level switch across an enter/exit cycle', () => {
    harness = openParsedTerminal()
    harness.feedChunk('\x1b[?1049h\x1b[2J\x1b[Hpager frame\x1b[K')
    expect(harness.bufferType()).toBe('alternate')
    expect(harness.switches()).toBe(1)
    harness.feedChunk('\x1b[?1049l')
    expect(harness.bufferType()).toBe('normal')
    expect(harness.switches()).toBe(2)
  })

  it('nets a single-chunk enter-and-exit cycle to zero buffer-change events', () => {
    harness = openParsedTerminal()
    // Fork difference vs xterm (which fired one event per switch mid-parse):
    // the facade polls the engine's alt-screen flag once per processed chunk,
    // so an intra-chunk cycle is invisible to onBufferChange. Pinned so the
    // atlas-recovery heuristics aren't assumed to see intra-chunk switches.
    harness.feedChunk('\x1b[?1049h\x1b[2J\x1b[Hpager frame\x1b[K\x1b[?1049l')
    expect(harness.bufferType()).toBe('normal')
    expect(harness.switches()).toBe(0)
  })

  it('tracks enter, split redraw, and exit from a real captured vim session', () => {
    harness = openParsedTerminal()
    // Captured from `vim package.json` (macOS, TERM=xterm-256color): startup chunk.
    harness.feedChunk(
      '\x1b[?1049h\x1b[>4;2m\x1b[?1h\x1b=\x1b[?2004h\x1b[?1004h\x1b[1;34r\x1b[?12h\x1b[?12l\x1b[22;2t\x1b[22;1t'
    )
    expect(harness.bufferType()).toBe('alternate')
    expect(harness.switches()).toBe(1)
    // Mid-session redraw where a 1024-byte PTY read split \x1b[30;5H in two.
    harness.feedChunk('"rules": {\x1b[29;15H\x1b[K\x1b[30')
    harness.feedChunk(
      ';5H  "js-combine-iterations": "off"\r\n    }\x1b[31;6H\x1b[K\x1b[33;1H\x1b[?25h'
    )
    expect(harness.bufferType()).toBe('alternate')
    expect(harness.switches()).toBe(1)
    // Vim quit: erase the status line and restore the normal buffer in one chunk.
    harness.feedChunk(
      '\x1b[23;2t\x1b[23;1t\x1b[34;1H\x1b[K\x1b[34;1H\x1b[?1004l\x1b[?2004l\x1b[?1l\x1b>\x1b[?1049l\x1b[?25h\x1b[>4;m'
    )
    expect(harness.bufferType()).toBe('normal')
    expect(harness.switches()).toBe(2)
  })
})
