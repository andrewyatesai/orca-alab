import { describe, expect, it } from 'vitest'
import { createAtermFacadeBuffer, type AtermBufferSource } from './aterm-facade-buffer'

// A grid cell as the engine reports it: lead graphemes carry their text, a wide
// lead flags cell_is_wide, and its trailing spacer (plus blanks) read as ''.
type Cell = { chars: string; wide?: boolean }

const COLS = 12

/** Build an AtermBufferSource over rows of explicit cells (the same read
 *  contract the wasm controller and the worker grid mirror implement). */
function sourceFromRows(rows: Cell[][]): AtermBufferSource {
  const cellAt = (row: number, col: number): Cell | undefined => rows[row]?.[col]
  const rowLen = (row: number): number | undefined => {
    const cells = rows[row]
    if (!cells) {
      return undefined
    }
    for (let col = cells.length - 1; col >= 0; col--) {
      if (cells[col].chars !== '') {
        return col + (cells[col].wide ? 2 : 1)
      }
    }
    return 0
  }
  return {
    gridSize: () => ({ cols: COLS, rows: rows.length }),
    isAltScreen: () => false,
    baseY: () => 0,
    displayOriginAbsolute: () => 0,
    cursorX: () => 0,
    cursorY: () => 0,
    rowIsWrapped: (row) => (row >= 0 && row < rows.length ? false : undefined),
    rowLen,
    rowText: (row) => (rows[row] ? rows[row].map((cell) => cell.chars).join('') || '' : undefined),
    cellText: (row, col) => cellAt(row, col)?.chars ?? '',
    cellIsWide: (row, col) => (rows[row] ? cellAt(row, col)?.wide === true : undefined)
  }
}

function cells(...specs: (string | [string])[]): Cell[] {
  // 'a' → normal cell; ['你'] → wide lead + its spacer cell.
  const out: Cell[] = []
  for (const spec of specs) {
    if (typeof spec === 'string') {
      out.push({ chars: spec })
    } else {
      out.push({ chars: spec[0], wide: true })
      out.push({ chars: '' }) // the wide cell's trailing spacer
    }
  }
  return out
}

function translate(
  rows: Cell[][],
  row: number,
  args: [boolean?, number?, number?]
): { text: string; columns: number[] } {
  const { buffer } = createAtermFacadeBuffer(() => sourceFromRows(rows))
  const line = buffer.active.getLine(row)
  if (!line) {
    throw new Error(`no line at ${row}`)
  }
  const columns: number[] = []
  const text = (
    line as unknown as {
      translateToString(t?: boolean, s?: number, e?: number, o?: number[]): string
    }
  ).translateToString(args[0], args[1], args[2], columns)
  return { text, columns }
}

describe('aterm facade buffer translateToString', () => {
  it('maps CJK wide cells to their lead COLUMN, not their char index', () => {
    // Columns: a=0 b=1 你=2(+3) 好=4(+5) c=6
    const row = cells('a', 'b', ['你'], ['好'], 'c')
    const { text, columns } = translate([row], 0, [false, 0, undefined])
    expect(text).toBe('ab你好c')
    // One column per output char + the end sentinel (xterm's addon contract).
    expect(columns).toEqual([0, 1, 2, 4, 6, 7])
  })

  it('slices by COLUMN indices across wide cells', () => {
    const row = cells('a', 'b', ['你'], ['好'], 'c')
    // startColumn 3 lands on 你's spacer → output starts at 好 (column 4).
    const { text, columns } = translate([row], 0, [false, 3, 7])
    expect(text).toBe('好c')
    expect(columns).toEqual([4, 6, 7])
  })

  it('maps both code units of a surrogate-pair grapheme to its lead column', () => {
    const row = cells('x', ['😀'], 'y')
    const { text, columns } = translate([row], 0, [false, 0, undefined])
    expect(text).toBe('x😀y')
    // 😀 is two code units; both map to column 1, y sits at column 3.
    expect(columns).toEqual([0, 1, 1, 3, 4])
  })

  it('pads blank cells to the requested end column and trims them on trimRight', () => {
    const row = cells(['あ'])
    const padded = translate([row], 0, [false, 0, 6])
    expect(padded.text).toBe('あ    ')
    expect(padded.columns).toEqual([0, 2, 3, 4, 5, 6])
    const trimmed = translate([row], 0, [true, 0, 6])
    expect(trimmed.text).toBe('あ')
    expect(trimmed.columns).toEqual([0, 2])
  })

  it('keeps the identity mapping on the all-ASCII fast path', () => {
    const row = cells('h', 'e', 'l', 'l', 'o')
    const { text, columns } = translate([row], 0, [false, 0, undefined])
    expect(text).toBe('hello')
    expect(columns).toEqual([0, 1, 2, 3, 4, 5])
    const sliced = translate([row], 0, [false, 1, 4])
    expect(sliced.text).toBe('ell')
    expect(sliced.columns).toEqual([1, 2, 3, 4])
  })
})
