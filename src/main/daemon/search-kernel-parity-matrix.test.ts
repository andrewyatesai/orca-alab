// Daemon-vs-wasm kernel differential parity matrix (deferred from Wave-4 4E).
//
// FEDERATED-SEARCH-DESIGN's standing policy is ONE matching kernel semantics
// everywhere: a query must mean the same thing whether it runs in the
// renderer's wasm engine (aterm-search via AtermTerminal.search) or the
// daemon/headless kernel (orca-terminal scrollback_search via the napi
// addon). This matrix drives BOTH kernels over the same byte corpus and
// pins:
//
//   PARITY (byte-identical match sets — rows, cols, lens, totals):
//     plain ASCII, ANSI-styled text, smart-case fold (incl. Turkish İ whose
//     lowercase EXPANDS, German ß, Greek final sigma), regex, invalid regex,
//     overlapping literal matches, trailing-blank trimming, wrap-boundary
//     non-matches, eviction-stable absolute rows.
//
//   DOCUMENTED DIVERGENCE (by design, fed §1 result model):
//     column UNITS on wide glyphs — the wasm kernel reports DISPLAY COLUMNS
//     (CJK/emoji occupy 2 cells) for highlight-rect math, while the daemon
//     kernel reports CHAR offsets into the text-only snippet ("text-only rows
//     have no wide-cell column identity — the renderer treats daemon rows as
//     approximate"). Rows and char lengths still agree; the delta is exactly
//     the extra cells of wide glyphs before the match, asserted below so any
//     OTHER unit drift fails the matrix.
import { readFileSync } from 'node:fs'
import { afterEach, beforeAll, describe, expect, it } from 'vitest'
import { loadRustTerminalBinding, type RustHeadlessTerminalHandle } from './rust-terminal-addon'

const ATERM_DIR = new URL('../../renderer/src/lib/pane-manager/aterm/', import.meta.url)
const FONT_URL = new URL('../../renderer/src/assets/fonts/jetbrains-mono.ttf', import.meta.url)

const ROWS = 6
const COLS = 40
// Matches ATERM_RENDERER_FONT_PX; the glyph raster is irrelevant to search.
const FONT_PX = 14

// Typed view of the renderer wasm engine's search surface. Imported
// DYNAMICALLY (path string, not a module specifier) because this test lives
// in the node tsconfig project, which deliberately excludes renderer sources.
type WasmSearchEngine = {
  process_str(text: string): void
  search(query: string, caseSensitive: boolean, isRegex: boolean): Uint32Array
  search_meta(
    query: string,
    caseSensitive: boolean,
    isRegex: boolean
  ): { incomplete: boolean; match_count: number }
  set_scrollback_limit(limit: number): void
  drain_scrollback_backlog(max: number): number
  free(): void
}

type WasmEngineModule = {
  initSync(opts: { module: Buffer }): unknown
  AtermTerminal: new (
    rows: number,
    cols: number,
    fontBytes: Uint8Array,
    fontPx: number,
    fg: number,
    bg: number,
    cursor: number,
    selection: number
  ) => WasmSearchEngine
}

let fontBytes: Uint8Array
let wasmEngine: WasmEngineModule

beforeAll(async () => {
  wasmEngine = (await import(
    new URL('aterm_wasm.js', ATERM_DIR).href
  )) as unknown as WasmEngineModule
  wasmEngine.initSync({ module: readFileSync(new URL('aterm_wasm_bg.wasm', ATERM_DIR)) })
  fontBytes = new Uint8Array(readFileSync(FONT_URL))
})

const openWasm: WasmSearchEngine[] = []
const openNapi: RustHeadlessTerminalHandle[] = []

afterEach(() => {
  for (const term of openWasm.splice(0)) {
    term.free()
  }
  for (const term of openNapi.splice(0)) {
    term.dispose()
  }
})

type KernelPair = { wasm: WasmSearchEngine; napi: RustHeadlessTerminalHandle }

function makeBoth(feed: string, opts: { rows?: number; cols?: number; scrollback?: number } = {}): KernelPair {
  const rows = opts.rows ?? ROWS
  const cols = opts.cols ?? COLS
  const wasm = new wasmEngine.AtermTerminal(
    rows,
    cols,
    fontBytes,
    FONT_PX,
    0xffffff,
    0x000000,
    0xffffff,
    0x334455
  )
  if (opts.scrollback !== undefined) {
    wasm.set_scrollback_limit(opts.scrollback)
  }
  const binding = loadRustTerminalBinding()
  expect(binding).not.toBeNull()
  const napi = new binding!.HeadlessTerminal(cols, rows, opts.scrollback ?? 10_000)
  wasm.process_str(feed)
  napi.write(Buffer.from(feed, 'utf8'))
  openWasm.push(wasm)
  openNapi.push(napi)
  return { wasm, napi }
}

type Span = { row: number; col: number; len: number }

function wasmMatches(term: WasmSearchEngine, query: string, cs: boolean, re: boolean): Span[] {
  // E1 staged promotion: settle so absolute rows agree with the napi side's
  // settle-first contract.
  while (term.drain_scrollback_backlog(4096) > 0) {
    // drain
  }
  const flat = term.search(query, cs, re)
  const out: Span[] = []
  for (let i = 0; i < flat.length; i += 3) {
    out.push({ row: flat[i], col: flat[i + 1], len: flat[i + 2] })
  }
  return out.sort((a, b) => a.row - b.row || a.col - b.col)
}

function napiMatches(
  term: RustHeadlessTerminalHandle,
  query: string,
  cs: boolean,
  re: boolean
): { spans: Span[]; lines: string[]; total: number; incomplete: boolean } {
  const outcome = term.searchScrollback(query, cs, re, 100_000)
  expect(typeof outcome.originRow).toBe('number')
  const spans = outcome.matches
    .map((m) => ({ row: (outcome.originRow ?? 0) + m.absRow, col: m.col, len: m.len, line: m.line }))
    .sort((a, b) => a.row - b.row || a.col - b.col)
  return {
    spans: spans.map(({ row, col, len }) => ({ row, col, len })),
    lines: spans.map((m) => m.line),
    total: outcome.total,
    incomplete: outcome.incomplete
  }
}

function expectParity(pair: KernelPair, query: string, cs = false, re = false): void {
  const wasm = wasmMatches(pair.wasm, query, cs, re)
  const napi = napiMatches(pair.napi, query, cs, re)
  expect(napi.spans).toEqual(wasm)
  expect(napi.total).toBe(wasm.length)
  const meta = pair.wasm.search_meta(query, cs, re)
  expect(napi.incomplete).toBe(meta.incomplete)
  expect(napi.total).toBe(meta.match_count)
}

describe('daemon-vs-wasm search kernel parity matrix', () => {
  it('plain ASCII literals', () => {
    const pair = makeBoth('alpha needle one\r\nnothing here\r\nbeta needle two\r\ntail\r\n')
    expectParity(pair, 'needle')
    expectParity(pair, 'nothing')
    expectParity(pair, 'absent-token')
  })

  it('ANSI-styled text matches on plain text in both kernels', () => {
    const pair = makeBoth('\x1b[1;32mgreen needle\x1b[0m\r\nplain needle\r\n\x1b[4munder\x1b[0mline\r\n')
    expectParity(pair, 'needle')
    expectParity(pair, 'underline')
  })

  it('case folding: default insensitive, explicit sensitive', () => {
    const pair = makeBoth('Mixed CASE line\r\nmixed case line\r\nMIXED CASE LINE\r\n')
    expectParity(pair, 'mixed case')
    expectParity(pair, 'CASE', true)
    expectParity(pair, 'mixed CASE', true)
  })

  it('Unicode folds agree: expanding Turkish İ, German ß, Greek final sigma', () => {
    const dotted = makeBoth('İstanbul needle\r\nistanbul needle\r\n')
    expectParity(dotted, 'İSTANBUL')
    // Offsets AFTER an expanding fold stay anchored to the original line.
    expectParity(dotted, 'NEEDLE')
    const sharp = makeBoth('straße here\r\nSTRASSE here\r\n')
    expectParity(sharp, 'straße')
    const sigma = makeBoth('λόγος test\r\n')
    expectParity(sigma, 'λόγοσ')
  })

  it('overlapping literal matches advance by one char in both kernels', () => {
    const pair = makeBoth('aaaa\r\nbanana banana\r\n')
    expectParity(pair, 'aa')
    expectParity(pair, 'ana')
  })

  it('regex: classes, alternation, case toggle, invalid pattern = zero matches', () => {
    const pair = makeBoth('error: code 137\r\nok: code 0\r\nERROR: code 9\r\n')
    expectParity(pair, 'code \\d+', false, true)
    expectParity(pair, '^(error|ok):', false, true)
    expectParity(pair, 'ERROR', true, true)
    expectParity(pair, '(unclosed', false, true)
    expectParity(pair, 'a{2,1}', false, true)
  })

  it('trailing blanks are trimmed identically (no phantom trailing-space matches)', () => {
    const pair = makeBoth('needle   \r\nx\r\n')
    expectParity(pair, 'needle ')
    expectParity(pair, 'needle')
  })

  it('neither kernel matches across a soft-wrap row boundary', () => {
    const pair = makeBoth(`${'B'.repeat(COLS - 3)}needle\r\nrescue needle\r\n`)
    // The needle is split across the wrapped rows: both kernels are row-text
    // scanners, so only the unwrapped occurrence matches.
    expectParity(pair, 'needle')
  })

  it('empty query yields zero matches in both kernels', () => {
    const pair = makeBoth('anything\r\n')
    expectParity(pair, '')
  })

  it('absolute rows stay in the SAME stable coordinate space across eviction', () => {
    const pair = makeBoth('', { rows: 1, cols: 20, scrollback: 5 })
    for (let i = 0; i < 12; i += 1) {
      const line = `row ${i}\r\n`
      pair.wasm.process_str(line)
      pair.napi.write(Buffer.from(line))
    }
    // Eviction happened on both sides; surviving rows keep identical abs rows.
    expectParity(pair, 'row 9')
    expectParity(pair, 'row')
  })

  describe('documented divergence: column units on wide glyphs', () => {
    // Display width of the corpus chars used below (CJK + emoji = 2 cells).
    const wideBefore = (line: string, chars: number): number => {
      let cells = 0
      let counted = 0
      for (const ch of line) {
        if (counted >= chars) {
          break
        }
        counted += 1
        cells += /[ᄀ-ᅟ⺀-꓏가-힣豈-﫿＀-｠]|\p{Extended_Pictographic}/u.test(
          ch
        )
          ? 2
          : 1
      }
      return cells
    }

    it('CJK/emoji: rows and char lens agree; wasm cols are display cells, daemon cols are chars', () => {
      for (const [line, query] of [
        ['你好 needle here', 'needle'],
        ['aa \u{1f600}\u{1f680} needle', 'needle']
      ] as const) {
        const pair = makeBoth(`${line}\r\n`)
        const wasm = wasmMatches(pair.wasm, query, false, false)
        const napi = napiMatches(pair.napi, query, false, false)
        expect(napi.spans).toHaveLength(1)
        expect(wasm).toHaveLength(1)
        expect(napi.spans[0].row).toBe(wasm[0].row)
        expect(napi.spans[0].len).toBe(wasm[0].len)
        // The unit difference is EXACTLY the wide-glyph cell surplus before
        // the match — anything else is a real kernel drift.
        expect(wasm[0].col).toBe(wideBefore(line, napi.spans[0].col))
        // The daemon's char offsets are self-consistent with its snippet
        // (offsets are Unicode code points, so slice code-point-wise —
        // JS .slice would count emoji as two UTF-16 units).
        expect(
          Array.from(napi.lines[0])
            .slice(napi.spans[0].col, napi.spans[0].col + napi.spans[0].len)
            .join('')
        ).toBe(query)
      }
    })
  })
})
