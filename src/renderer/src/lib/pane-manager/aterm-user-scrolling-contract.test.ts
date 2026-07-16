/**
 * Contract test for aterm's native user-scrolling ownership, pinned against
 * the REAL wasm engine (fork port of the upstream @xterm/headless contract
 * pin; the engine artifact itself is pinned by check:aterm-pin).
 *
 * Orca's live PTY write path performs NO per-write scroll-intent enforcement
 * (see writeBackgroundTerminalChunk in pane-terminal-output-scheduler.ts). It
 * relies on two behaviors this file pins:
 *   - ENGINE: a scrolled-up viewport stays content-stable while output is
 *     written (SCR-1 repin: display_offset advances as lines push in so
 *     display_origin_absolute — the facade's viewportY — stays put).
 *   - PUMP: follow-at-bottom lives in aterm-process-pump.ts, not the engine:
 *     wasAtBottom = display_offset === 0 before process, scroll_to_bottom()
 *     after if the parse moved it. pumpChunk() mirrors that exact rule.
 * App-side enforcement is scoped to structural operations (snapshot replay,
 * remount, fit reflow) in terminal-scroll-intent.ts.
 *
 * If an aterm upgrade breaks any assertion here, the live write path loses
 * its follow/pin semantics silently — fix the write path before repinning.
 *
 * Dropped vs upstream: the xterm version-pin test (check:aterm-pin replaces
 * it) and the coreService.onUserInput / onData-ordering tests (the fork
 * distinguishes real user input from parser auto-replies at the
 * pty-connection seam, not inside the engine).
 */
import { readFileSync } from 'node:fs'
import { afterEach, beforeAll, describe, expect, it } from 'vitest'
import { initSync, AtermTerminal } from '@/lib/pane-manager/aterm/aterm_wasm.js'
import { buildAtermEngineReads } from '@/lib/pane-manager/aterm/aterm-engine-reads'
import {
  createAtermFacadeBuffer,
  type AtermBufferSource
} from '@/lib/pane-manager/aterm/aterm-facade-buffer'
import { ATERM_RENDERER_FONT_PX } from '@/lib/pane-manager/aterm/aterm-pane-controller-types'
import { clearTerminalScrollbackAndFollowOutput } from './terminal-scrollback-clear'
import { getTerminalScrollIntentKind } from './terminal-scroll-intent'

const ATERM_DIR = new URL('./aterm/', import.meta.url)
const FONT_URL = new URL('../../assets/fonts/jetbrains-mono.ttf', import.meta.url)

// The engine's REAL retention boundary: this wasm build attaches no tiered
// deep-history store (Terminal::new -> Grid::new, scrollback: None), so
// set_scrollback_limit cannot shrink retention — the in-grid ring's fixed
// 10_000-line cap IS the live trim boundary the write path runs against.
const RING_SCROLLBACK_CAP = 10_000

let fontBytes: Uint8Array

beforeAll(() => {
  // Real engine, loaded headlessly: initSync + on-disk bytes replaces the
  // browser fetch path (load-aterm.ts) that node tests can't use.
  initSync({ module: readFileSync(new URL('aterm_wasm_bg.wasm', ATERM_DIR)) })
  fontBytes = new Uint8Array(readFileSync(FONT_URL))
})

const openTerms: AtermTerminal[] = []

afterEach(() => {
  for (const term of openTerms.splice(0)) {
    term.free()
  }
})

function openEngine(rows: number, cols: number): AtermTerminal {
  const term = new AtermTerminal(
    rows,
    cols,
    fontBytes,
    ATERM_RENDERER_FONT_PX,
    0xffffff,
    0x000000,
    0xffffff,
    0x334455
  )
  openTerms.push(term)
  return term
}

/** One live PTY chunk through the fork's write-path scroll rule. The engine
 *  natively repins a scrolled-up viewport; the follow-at-bottom half of the
 *  contract is the process pump's (aterm-process-pump.ts), so mirror its
 *  exact rule: re-follow ONLY when display_offset was 0 before the parse. */
function pumpChunk(term: AtermTerminal, data: string): void {
  const wasAtBottom = term.display_offset === 0
  term.process_str(data)
  if (wasAtBottom && term.display_offset !== 0) {
    term.scroll_to_bottom()
  }
}

function pumpLines(term: AtermTerminal, count: number, label: string, start = 0): void {
  for (let i = start; i < start + count; i += 1) {
    pumpChunk(term, `${label}${i}\r\n`)
  }
}

/** Flood fill in one chunk (a PTY burst): the engine repins per process()
 *  batch, so one big chunk and per-line chunks pin the same contract. */
function pumpLineBurst(term: AtermTerminal, count: number, label: string, start = 0): void {
  let chunk = ''
  for (let i = start; i < start + count; i += 1) {
    chunk += `${label}${i}\r\n`
  }
  pumpChunk(term, chunk)
}

describe('aterm native user-scrolling contract (real wasm engine)', () => {
  it('keeps a scrolled-up viewport stable while output is written', () => {
    const term = openEngine(10, 40)
    pumpLines(term, 30, 'line')
    expect(term.display_offset).toBe(0)
    expect(term.display_origin_absolute).toBe(term.base_y)

    // Positive scroll_lines = toward OLDER lines (the facade negates xterm's
    // scrollLines amount, see aterm-terminal-facade.ts scrollLines).
    term.scroll_lines(5)
    expect(term.display_offset).toBe(5)
    const pinnedOrigin = term.display_origin_absolute
    expect(pinnedOrigin).toBe(term.base_y - 5)
    const pinnedTop = term.row_text(0)
    expect(pinnedTop).toBe('line16')

    pumpLines(term, 10, 'more')
    // SCR-1 repin: display_offset advances as lines push in so the visible
    // content (origin + rows) stays fixed — xterm's isUserScrolling pin.
    expect(term.display_origin_absolute).toBe(pinnedOrigin)
    expect(term.row_text(0)).toBe(pinnedTop)
    expect(term.display_offset).toBe(15)
    expect(term.base_y).toBe(pinnedOrigin + 15)
  })

  it('treats a viewport one row above bottom as user-scrolling through output', () => {
    const term = openEngine(10, 40)
    pumpLines(term, 30, 'line')

    // Exactly one row up: the pump's at-bottom check is display_offset === 0
    // EXACTLY, so offset 1 must pin, not follow.
    term.scroll_lines(1)
    expect(term.display_offset).toBe(1)
    const pinnedOrigin = term.display_origin_absolute
    const pinnedTop = term.row_text(0)
    expect(pinnedTop).toBe('line20')

    pumpLines(term, 5, 'more')
    expect(term.display_origin_absolute).toBe(pinnedOrigin)
    expect(term.row_text(0)).toBe(pinnedTop)
    expect(term.display_offset).toBe(6)
  })

  it('follows output at the bottom and re-follows after scrolling back down', () => {
    const term = openEngine(10, 40)
    pumpLines(term, 30, 'line')

    pumpLines(term, 5, 'tail')
    expect(term.display_offset).toBe(0)
    expect(term.display_origin_absolute).toBe(term.base_y)
    // Latest line visible just above the cursor line at the live bottom.
    expect(term.row_text(8)).toBe('tail4')

    term.scroll_lines(5)
    term.scroll_to_bottom()
    pumpLines(term, 5, 'after')
    expect(term.display_offset).toBe(0)
    expect(term.display_origin_absolute).toBe(term.base_y)
    expect(term.row_text(8)).toBe('after4')
  })

  it('walks a pinned viewport content-stably when scrollback trims', () => {
    const term = openEngine(5, 20)
    pumpLineBurst(term, RING_SCROLLBACK_CAP + 20, 'x')
    expect(term.display_offset).toBe(0)
    const fullBase = term.base_y

    // Trimming is real at the ring cap: the oldest retained line has walked
    // past x0 (x0..x15 are gone), so the assertions below cross a live trim.
    term.scroll_to_top()
    expect(term.display_offset).toBe(RING_SCROLLBACK_CAP)
    expect(term.display_origin_absolute).toBe(fullBase - RING_SCROLLBACK_CAP)
    expect(term.row_text(0)).toBe(`x${fullBase - RING_SCROLLBACK_CAP}`)

    term.scroll_to_bottom()
    term.scroll_lines(10)
    const pinnedOrigin = term.display_origin_absolute
    const pinnedTop = term.row_text(0)

    pumpLines(term, 10, 'x', RING_SCROLLBACK_CAP + 20)
    // Upstream xterm renumbers rows on trim, so its baseY froze at the cap
    // while the pinned viewportY walked down (viewportY == max(0, pinnedY -
    // trimmed)) to keep content still. aterm's absolute rows are monotonic —
    // nothing renumbers — so the SAME content identity reads as a FIXED
    // display_origin_absolute (the facade's viewportY) while base_y grows.
    expect(term.display_origin_absolute).toBe(pinnedOrigin)
    expect(term.row_text(0)).toBe(pinnedTop)
    expect(term.base_y).toBe(fullBase + 10)
    expect(term.display_offset).toBe(20)
  })

  it('clamps a top-pinned viewport at the retention cap once its rows are evicted', () => {
    const term = openEngine(5, 20)
    pumpLineBurst(term, RING_SCROLLBACK_CAP + 20, 'x')
    term.scroll_to_top()
    expect(term.display_offset).toBe(RING_SCROLLBACK_CAP)
    const topOrigin = term.display_origin_absolute
    expect(term.row_text(0)).toBe(`x${topOrigin}`)

    pumpLines(term, 20, 'x', RING_SCROLLBACK_CAP + 20)
    // The pinned rows themselves were trimmed: the offset clamps at the
    // retention cap (upstream's max(0, ...) floor) and the content walks.
    expect(term.display_offset).toBe(RING_SCROLLBACK_CAP)
    expect(term.display_origin_absolute).toBe(topOrigin + 20)
    expect(term.row_text(0)).toBe(`x${topOrigin + 20}`)
  })

  it('resets user-scrolling when a pinned scrollback is cleared', () => {
    const term = openEngine(10, 40)
    // The production seam clearTerminalScrollbackAndFollowOutput runs against:
    // real engine reads behind buffer.active (viewportY/baseY), clear()
    // mirroring the facade's clear() (process of xterm's clear-equivalent
    // control string), and the real engine bottom scroll.
    const reads = buildAtermEngineReads(
      term,
      { dpr: 1, cellWidth: 0, cellHeight: 0 },
      () => undefined,
      () => false
    )
    const source: AtermBufferSource = { ...reads, gridSize: () => ({ cols: 40, rows: 10 }) }
    const { buffer } = createAtermFacadeBuffer(() => source)
    const terminal = {
      buffer,
      // aterm-terminal-facade clear(): CSI 2J (screen) + 3J (scrollback) + H.
      clear: (): void => term.process_str('[2J[3J[H'),
      scrollToBottom: (): void => term.scroll_to_bottom()
    }

    pumpLines(term, 30, 'line')
    term.scroll_lines(5)
    expect(buffer.active.viewportY).toBe(buffer.active.baseY - 5)

    clearTerminalScrollbackAndFollowOutput(terminal)
    // Fork difference vs xterm (which zeroes baseY): aterm's absolute rows are
    // monotonic, so "cleared + following" reads as viewportY == baseY with no
    // retained history — not zeroed coordinates.
    expect(term.display_offset).toBe(0)
    expect(buffer.active.viewportY).toBe(buffer.active.baseY)
    // The pin is really gone: there is no scrollback left to re-enter.
    term.scroll_lines(5)
    expect(term.display_offset).toBe(0)
    // App-side stand-in for xterm's isUserScrolling === false after clear.
    expect(getTerminalScrollIntentKind(terminal)).toBe('followOutput')

    pumpLines(term, 15, 'after-clear')
    expect(term.display_offset).toBe(0)
    expect(buffer.active.viewportY).toBe(buffer.active.baseY)
  })
})
