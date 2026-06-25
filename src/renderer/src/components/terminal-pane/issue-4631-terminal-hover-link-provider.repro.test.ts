import { performance } from 'node:perf_hooks'
import { describe, expect, it, vi } from 'vitest'
import type { IDisposable } from '../../lib/pane-manager/aterm/terminal-types'
import {
  createAtermFacadeBuffer,
  type AtermBufferSource,
  type AtermFacadeBuffer
} from '@/lib/pane-manager/aterm/aterm-facade-buffer'
import type { PaneManager } from '@/lib/pane-manager/pane-manager'
import { extractTerminalFileLinks } from '@/lib/terminal-links'
import { createFilePathLinkProvider, getTerminalFileOpenHint } from './terminal-link-handlers'
import { buildWrappedLogicalLine } from './wrapped-terminal-link-ranges'

vi.mock('@/store', () => ({
  useAppStore: {
    getState: () => ({
      settings: undefined,
      setActiveWorktree: vi.fn(),
      createBrowserTab: vi.fn(),
      openFile: vi.fn(),
      setPendingEditorReveal: vi.fn()
    })
  }
}))

vi.mock('@/lib/language-detect', () => ({
  detectLanguage: () => 'plaintext'
}))

vi.mock('@/lib/worktree-activation', () => ({
  activateAndRevealWorktree: vi.fn()
}))

vi.mock('@/lib/connection-context', () => ({
  getConnectionId: () => null
}))

const COLS = 80

/** An in-memory AtermBufferSource over a REAL stored grid of wrapped rows. This
 *  is the same row/cell read contract the wasm controller implements in
 *  production (gridSize/isAltScreen/baseY/displayOriginAbsolute/cursor/rowIsWrapped/
 *  rowLen/rowText/cellText/cellIsWide), so the facade builds genuine IBufferLines
 *  and the link code under test runs unchanged. baseY/displayOrigin are 0 so the
 *  whole grid is on-screen and absolute index == display row. */
function createWrappedGridSource(rows: string[], wrappedFlags: boolean[]): AtermBufferSource {
  const rowAt = (row: number): string | undefined =>
    row >= 0 && row < rows.length ? rows[row] : undefined
  return {
    gridSize: () => ({ cols: COLS, rows: rows.length }),
    isAltScreen: () => false,
    baseY: () => 0,
    displayOriginAbsolute: () => 0,
    cursorX: () => 0,
    cursorY: () => 0,
    rowIsWrapped: (row) => (row >= 0 && row < rows.length ? wrappedFlags[row] : undefined),
    rowLen: (row) => rowAt(row)?.length,
    rowText: (row) => rowAt(row),
    cellText: (row, col) => rowAt(row)?.[col] ?? '',
    cellIsWide: (row) => (row >= 0 && row < rows.length ? false : undefined)
  }
}

function buildWrappedFacadeBuffer(rowCount: number): AtermFacadeBuffer {
  // One short logical line (79 'a's) then `rowCount` full rows of 80 'b's that
  // soft-wrap into a single logical line — the original issue-4631 payload.
  const rows = ['a'.repeat(COLS - 1), ...Array.from({ length: rowCount }, () => 'b'.repeat(COLS))]
  // Row 0 (a's) and row 1 (first b row) start logical lines; rows 2..N are
  // wrapped continuations the wrap-walk must traverse.
  const wrappedFlags = rows.map((_row, index) => index >= 2)
  const source = createWrappedGridSource(rows, wrappedFlags)
  return createAtermFacadeBuffer(() => source).buffer
}

function configuredRowCount(): number {
  const raw = process.env.ORCA_4631_WRAP_ROWS
  if (!raw) {
    return 50_000
  }
  const parsed = Number.parseInt(raw, 10)
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 50_000
}

function logStage(stage: string, details: Record<string, unknown>): void {
  if (process.env.ORCA_LOG_4631_REPRO !== '1') {
    return
  }
  process.stderr.write(`${JSON.stringify({ issue: 4631, stage, ...details })}\n`)
}

describe('issue 4631 terminal hover link-provider repro', () => {
  it('keeps hover link detection bounded for a huge soft-wrapped terminal line', async () => {
    vi.stubGlobal('navigator', { userAgent: 'Macintosh' })
    vi.stubGlobal('window', {
      api: {
        shell: {
          pathExists: vi.fn().mockResolvedValue(false)
        }
      }
    })

    const rowCount = configuredRowCount()
    const buildBufferStart = performance.now()
    const buffer = buildWrappedFacadeBuffer(rowCount)
    logStage('buildFacadeBuffer', {
      rowCount,
      elapsedMs: Math.round(performance.now() - buildBufferStart),
      bufferBaseY: buffer.active.baseY,
      bufferLength: buffer.active.length
    })

    const targetBufferLine = 2
    const buildStart = performance.now()
    const logicalLine = buildWrappedLogicalLine(buffer.active, targetBufferLine)
    const buildElapsedMs = performance.now() - buildStart
    logStage('buildWrappedLogicalLine', {
      rowCount,
      elapsedMs: Math.round(buildElapsedMs),
      logicalTextLength: logicalLine?.text.length ?? null,
      wrappedRows: logicalLine?.rows.length ?? null
    })

    const extractStart = performance.now()
    const directLinks = logicalLine ? extractTerminalFileLinks(logicalLine.text) : []
    const extractElapsedMs = performance.now() - extractStart
    logStage('extractTerminalFileLinks', {
      rowCount,
      elapsedMs: Math.round(extractElapsedMs),
      directLinkCount: directLinks.length
    })

    // A minimal pane whose terminal exposes the real facade buffer active + the
    // real grid geometry; the provider reads pane.terminal.buffer.active and cols/rows.
    const terminal = { buffer, cols: COLS, rows: rowCount + 1 }
    const pane = { id: 1, terminal }
    const managerRef = {
      current: { getPanes: () => [pane] } as unknown as PaneManager
    }
    const provider = createFilePathLinkProvider(
      1,
      {
        worktreeId: 'wt-1',
        worktreePath: '/repo',
        startupCwd: '/repo',
        managerRef,
        linkProviderDisposablesRef: { current: new Map<number, IDisposable>() },
        pathExistsCache: new Map()
      },
      { textContent: '', style: { display: '' } } as unknown as HTMLElement,
      getTerminalFileOpenHint()
    )

    const start = performance.now()
    await new Promise<void>((resolve) => {
      provider.provideLinks(targetBufferLine, () => resolve())
    })
    const elapsedMs = performance.now() - start

    logStage('createFilePathLinkProvider.provideLinks', {
      cols: COLS,
      rowCount,
      elapsedMs: Math.round(elapsedMs),
      bufferBaseY: buffer.active.baseY,
      targetBufferLine
    })

    expect(buildElapsedMs + extractElapsedMs + elapsedMs).toBeLessThan(100)
  }, 120_000)
})
