/**
 * @vitest-environment happy-dom
 */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createAtermWorkerQueryChannel } from './aterm-worker-query-channel'
import { createWorkerTerminal } from './aterm-worker-terminal'
import { attachAtermSelectionInput } from './aterm-selection-input'
import { attachAtermLinkInput } from './aterm-link-input'
import type { EngineHandle } from './aterm-worker-engine-build'
import type { AtermTerminal } from './aterm_wasm.js'

const flush = (): Promise<void> => new Promise((r) => setTimeout(r, 0))

// ── Finding 1: dispose-leak — the query channel must never leave an awaiter hanging ──
describe('aterm worker query channel (dispose-leak)', () => {
  afterEach(() => vi.useRealTimers())

  it('resolves a pending serialize by id', async () => {
    let sentId = -1
    const ch = createAtermWorkerQueryChannel((cmd) => {
      if (cmd.type === 'query') {
        sentId = cmd.id
      }
    })
    const p = ch.serializeAsync()
    ch.resolve(sentId, 'BLOB')
    await expect(p).resolves.toBe('BLOB')
  })

  it('settles EVERY in-flight query to "" on dispose (worker terminated at quit)', async () => {
    const ch = createAtermWorkerQueryChannel(() => undefined)
    const a = ch.serializeAsync()
    const b = ch.serializeScrollbackAsync()
    const c = ch.selectionTextAsync()
    ch.dispose()
    // Without the dispose-flush these awaiters hang forever (a quit-time Promise.all stalls).
    await expect(Promise.all([a, b, c])).resolves.toEqual(['', '', ''])
  })

  it('times out a dropped queryResult to "" instead of hanging', async () => {
    vi.useFakeTimers()
    const ch = createAtermWorkerQueryChannel(() => undefined)
    const p = ch.serializeAsync()
    vi.advanceTimersByTime(5000)
    await expect(p).resolves.toBe('')
  })
})

// ── Finding 3: legacy mouse — bytes ≥ 0x80 must survive (no ASCII strip) ──
describe('aterm worker mouseEncode (legacy 1000/1002/1003 high bytes)', () => {
  it('preserves every report byte 0..255 (Latin-1), not just ASCII', () => {
    // Legacy X10 press at column/row 100: ESC [ M <btn+32> <col+33> <row+33>; 32+100+1=133=0x85.
    const bytes = Uint8Array.from([0x1b, 0x5b, 0x4d, 0x20, 0x85, 0x85])
    const handle = {
      kind: 'cpu',
      engine: {
        encode_mouse_press: () => bytes,
        encode_mouse_release: () => bytes,
        encode_mouse_motion: () => bytes,
        encode_mouse_wheel: () => bytes
      }
    } as unknown as EngineHandle
    const term = createWorkerTerminal(handle)
    const out = term.mouseEncode('press', 100, 100, 0, 0, false)
    // decodeReply would drop the two 0x85 bytes → length 4; the fix keeps all 6.
    expect(out.length).toBe(6)
    expect([...out].map((ch) => ch.charCodeAt(0))).toEqual([...bytes])
  })
})

describe('aterm worker buildState selectionText gating (per-frame clone avoidance)', () => {
  type Range = { start_x: number; start_y: number; end_x: number; end_y: number } | undefined
  function makeBuildStateHandle(range: Range, text: string) {
    let selRange = range
    let selText = text
    const selectionText = vi.fn(() => selText)
    const engine = {
      cell_width: 8,
      cell_height: 16,
      display_offset: 0,
      display_origin_absolute: 0,
      cursor_x: 0,
      cursor_y: 0,
      cursor_style: 1,
      base_y: 0,
      is_alt_screen: false,
      bracketed_paste_mode: false,
      is_mouse_tracking: false,
      mouse_wants_motion: false,
      mouse_wants_any_motion: false,
      is_focus_event_mode: false,
      is_color_scheme_updates_mode: false,
      is_app_cursor_mode: false,
      search_display_origin: 0,
      title: () => null,
      selection_range: () => selRange,
      selection_text: selectionText,
      resize: () => undefined
    }
    const handle = {
      kind: 'cpu',
      engine,
      framebuffer: () => ({ width: 80, height: 48 }),
      render: () => undefined,
      search: () => new Uint32Array(0)
    } as unknown as EngineHandle
    return {
      handle,
      selectionText,
      setSelection: (r: Range, t: string) => {
        selRange = r
        selText = t
      }
    }
  }

  it('omits selectionText (and skips re-materializing it) when the range is unchanged', () => {
    const { handle, selectionText } = makeBuildStateHandle(
      { start_x: 1, start_y: 0, end_x: 5, end_y: 0 },
      'hello'
    )
    const term = createWorkerTerminal(handle)
    expect(term.buildState().selectionText).toBe('hello')
    expect(selectionText).toHaveBeenCalledTimes(1)
    // Same range next frame → omitted + NOT re-materialized over the wasm boundary.
    expect(term.buildState().selectionText).toBeUndefined()
    expect(selectionText).toHaveBeenCalledTimes(1)
  })

  it('re-emits selectionText when the selection range changes', () => {
    const { handle, setSelection } = makeBuildStateHandle(undefined, '')
    const term = createWorkerTerminal(handle)
    expect(term.buildState().selectionText).toBe('') // first frame: empty selection
    setSelection({ start_x: 0, start_y: 0, end_x: 3, end_y: 0 }, 'abc')
    expect(term.buildState().selectionText).toBe('abc')
    // Clearing the selection re-emits one final '' so the main side resets.
    setSelection(undefined, '')
    expect(term.buildState().selectionText).toBe('')
  })
})

// ── P7: churn throttle withholds ONLY dirtyRows — every STATE scalar stays live ──
describe('aterm worker buildState P7 churn throttle (scalars never throttled)', () => {
  afterEach(() => vi.restoreAllMocks())

  type Range = { start_x: number; start_y: number; end_x: number; end_y: number } | undefined
  function makeChurnHandle() {
    const rowsText = ['aa', 'bb']
    let selRange: Range
    let selText = ''
    const engine = {
      cell_width: 8,
      cell_height: 16,
      display_offset: 0,
      display_origin_absolute: 0,
      cursor_x: 0,
      cursor_y: 0,
      cursor_style: 1,
      base_y: 0,
      is_alt_screen: false,
      bracketed_paste_mode: false,
      is_mouse_tracking: false,
      mouse_wants_motion: false,
      mouse_wants_any_motion: false,
      is_focus_event_mode: false,
      is_color_scheme_updates_mode: false,
      is_app_cursor_mode: false,
      search_display_origin: 0,
      title: () => null,
      selection_range: () => selRange,
      selection_text: () => selText,
      resize: () => undefined,
      row_text: (r: number) => rowsText[r],
      row_is_wrapped: () => false,
      row_len: (r: number) => rowsText[r]?.length ?? 0,
      cell_is_wide: () => false
    }
    const handle = {
      kind: 'cpu',
      engine,
      framebuffer: () => ({ width: 80, height: 48 }),
      render: () => undefined,
      search: () => new Uint32Array(0)
    } as unknown as EngineHandle
    return {
      handle,
      engine,
      setRow: (r: number, text: string) => {
        rowsText[r] = text
      },
      setSelection: (r: Range, t: string) => {
        selRange = r
        selText = t
      }
    }
  }

  it('withholds dirtyRows mid-fling while offset/cursor/selection stay live, then settle re-syncs', () => {
    vi.spyOn(performance, 'now').mockReturnValue(0) // freeze the churn rate-limit window
    const { handle, engine, setRow, setSelection } = makeChurnHandle()
    const term = createWorkerTerminal(handle)
    term.resize(2, 4)
    expect(term.buildState().dirtyRows).toHaveLength(2) // first frame: full mirror
    engine.display_offset = 5
    term.buildState() // churn frame 1 (a single wheel notch) → still exports
    engine.display_offset = 10
    engine.cursor_x = 7
    engine.cursor_y = 1
    setRow(0, 'cc')
    setSelection({ start_x: 0, start_y: 0, end_x: 2, end_y: 0 }, 'cc')
    const throttled = term.buildState() // churn frame 2 → mirror withheld
    expect(throttled.dirtyRows).toEqual([])
    expect(term.gridMirrorStale()).toBe(true)
    // The authoritative scalars are NEVER throttled (Codex P7 required change).
    expect(throttled.displayOffset).toBe(10)
    expect(throttled.cursorX).toBe(7)
    expect(throttled.cursorY).toBe(1)
    expect(throttled.selectionRange).toEqual({ startX: 0, startY: 0, endX: 2, endY: 0 })
    expect(throttled.selectionText).toBe('cc')
    // Settle (offset stable): the mirror re-syncs the changed row in full.
    const settled = term.buildState()
    expect(settled.dirtyRows.map((r) => r.text)).toEqual(['cc'])
    expect(term.gridMirrorStale()).toBe(false)
  })
})

// ── R7: hover posts a render-free STATE only when the link/cursor OUTCOME changes ──
describe('aterm worker setHover (render-free hover outcome gating)', () => {
  type Hit = { url: string; kind: number; start_col: number; end_col: number }
  function makeTerm(linkAt: (row: number, col: number) => Hit | undefined) {
    const engine = { link_at: vi.fn(linkAt) }
    return createWorkerTerminal({ kind: 'cpu', engine } as unknown as EngineHandle)
  }

  it('reports change only on entering + leaving a link, not while sweeping within it', () => {
    const hit: Hit = { url: 'u', kind: 1, start_col: 2, end_col: 6 }
    const term = makeTerm((_row, col) => (col >= 2 && col <= 6 ? hit : undefined))
    expect(term.setHover({ row: 0, col: 3 })).toBe(true) // entered the link → post
    expect(term.setHover({ row: 0, col: 4 })).toBe(false) // same link span → no post
    expect(term.setHover({ row: 0, col: 5 })).toBe(false) // still same span → no post
    expect(term.setHover({ row: 0, col: 9 })).toBe(true) // left the link → post (clear)
    expect(term.setHover({ row: 0, col: 10 })).toBe(false) // still link-free → no post
    expect(term.setHover(null)).toBe(false) // already cleared → no post
  })

  it('reports change when crossing from one link to a different one', () => {
    const linkA: Hit = { url: 'a', kind: 1, start_col: 0, end_col: 2 }
    const linkB: Hit = { url: 'b', kind: 2, start_col: 4, end_col: 6 }
    const term = makeTerm((_row, col) => (col <= 2 ? linkA : col >= 4 ? linkB : undefined))
    expect(term.setHover({ row: 0, col: 1 })).toBe(true) // link A
    expect(term.setHover({ row: 0, col: 5 })).toBe(true) // link B (different url/kind/span)
  })

  it('sweeping only link-free cells never reports a change (posts nothing)', () => {
    const term = makeTerm(() => undefined)
    expect(term.setHover({ row: 0, col: 0 })).toBe(false) // matches the initial no-hover state
    expect(term.setHover({ row: 0, col: 1 })).toBe(false)
    expect(term.setHover({ row: 5, col: 9 })).toBe(false)
  })
})

// ── Shared DOM harness for the input handlers ──
function makeCanvas(): HTMLCanvasElement {
  const canvas = document.createElement('canvas')
  document.body.appendChild(canvas)
  return canvas
}

// ── Finding 2: copy-on-select over the worker seam ──
describe('aterm selection copy-on-select (worker async text)', () => {
  it('drag mouseup copies the ASYNC selection text (sync snapshot is stale)', async () => {
    const onCopy = vi.fn()
    const term = {
      is_mouse_tracking: false,
      selection_text: () => '', // lagging worker snapshot right after selection_finish
      selection_clear: vi.fn(),
      selection_start: vi.fn(),
      selection_extend: vi.fn(),
      selection_finish: vi.fn(),
      selectionTextAsync: () => Promise.resolve('dragged text')
    } as unknown as AtermTerminal
    const canvas = makeCanvas()
    attachAtermSelectionInput({
      canvas,
      term,
      dpr: 1,
      cellWidth: 10,
      cellHeight: 10,
      redraw: vi.fn(),
      isDisposed: () => false,
      onCopy,
      getCopyOnSelect: () => true
    })
    canvas.dispatchEvent(
      new MouseEvent('mousedown', { button: 0, detail: 1, clientX: 5, clientY: 5 })
    )
    window.dispatchEvent(new MouseEvent('mouseup'))
    await flush()
    // Before the fix copySelection() read the stale '' → nothing copied.
    expect(onCopy).toHaveBeenCalledWith('dragged text')
  })

  it('double-click word copies the ASYNC text even though selection_word returns undefined', async () => {
    const onCopy = vi.fn()
    const term = {
      is_mouse_tracking: false,
      selection_text: () => '',
      selection_clear: vi.fn(),
      selection_start: vi.fn(),
      selection_extend: vi.fn(),
      selection_finish: vi.fn(),
      selection_word: () => undefined, // worker facade posts + returns undefined
      selection_line: () => undefined,
      selectionTextAsync: () => Promise.resolve('word')
    } as unknown as AtermTerminal
    const canvas = makeCanvas()
    attachAtermSelectionInput({
      canvas,
      term,
      dpr: 1,
      cellWidth: 10,
      cellHeight: 10,
      redraw: vi.fn(),
      isDisposed: () => false,
      onCopy,
      getCopyOnSelect: () => true
    })
    canvas.dispatchEvent(
      new MouseEvent('mousedown', { button: 0, detail: 2, clientX: 5, clientY: 5 })
    )
    await flush()
    expect(onCopy).toHaveBeenCalledWith('word')
  })

  it('in-process path is unchanged: copies the SYNC return / selection_text', () => {
    const onCopy = vi.fn()
    const term = {
      is_mouse_tracking: false,
      selection_text: () => 'sync drag',
      selection_clear: vi.fn(),
      selection_start: vi.fn(),
      selection_extend: vi.fn(),
      selection_finish: vi.fn(),
      selection_word: () => 'sync word',
      selection_line: () => 'sync line'
      // no selectionTextAsync → in-process engine
    } as unknown as AtermTerminal
    const canvas = makeCanvas()
    attachAtermSelectionInput({
      canvas,
      term,
      dpr: 1,
      cellWidth: 10,
      cellHeight: 10,
      redraw: vi.fn(),
      isDisposed: () => false,
      onCopy,
      getCopyOnSelect: () => true
    })
    canvas.dispatchEvent(
      new MouseEvent('mousedown', { button: 0, detail: 2, clientX: 5, clientY: 5 })
    )
    expect(onCopy).toHaveBeenCalledWith('sync word') // synchronous, no await needed
  })
})

// ── Finding 4: link hover cursor + click activation over the worker seam ──
describe('aterm link input (worker hover/click)', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('Cmd/Ctrl+click resolves the link via the ASYNC query and opens it', async () => {
    const openUrl = vi.fn()
    const term = {
      is_alt_screen: false,
      is_mouse_tracking: false,
      link_at: () => undefined, // lagging snapshot has no hit yet
      linkAtAsync: () =>
        Promise.resolve({ url: 'https://example.test', kind: 1, start_col: 0, end_col: 3 }),
      clearHover: vi.fn()
    } as unknown as AtermTerminal
    const canvas = makeCanvas()
    attachAtermLinkInput({
      canvas,
      term,
      metrics: { dpr: 1, cellWidth: 10, cellHeight: 10 },
      redraw: vi.fn(),
      isDisposed: () => false,
      openUrl,
      getFileLinkOpener: () => null
    })
    canvas.dispatchEvent(
      new MouseEvent('click', { button: 0, metaKey: true, ctrlKey: true, clientX: 5, clientY: 5 })
    )
    await flush()
    expect(openUrl).toHaveBeenCalledWith('https://example.test', { forceSystemBrowser: false })
  })

  it('worker path does NOT write the canvas cursor on hover (loader owns it)', () => {
    vi.stubGlobal('requestAnimationFrame', (cb: () => void) => {
      cb()
      return 1
    })
    const term = {
      is_alt_screen: false,
      is_mouse_tracking: false,
      // Even when the (stale) sync snapshot reports a hit, the worker path must not set
      // 'pointer' here — the loader drives the cursor from state.hoverCursor.
      link_at: () => ({ url: 'u', kind: 1, start_col: 0, end_col: 3 }),
      linkAtAsync: () => Promise.resolve(null),
      clearHover: vi.fn()
    } as unknown as AtermTerminal
    const canvas = makeCanvas()
    attachAtermLinkInput({
      canvas,
      term,
      metrics: { dpr: 1, cellWidth: 10, cellHeight: 10 },
      redraw: vi.fn(),
      isDisposed: () => false,
      openUrl: vi.fn(),
      getFileLinkOpener: () => null
    })
    canvas.dispatchEvent(new MouseEvent('mousemove', { clientX: 5, clientY: 5 }))
    expect(canvas.style.cursor).toBe('') // not 'pointer'
  })

  it('in-process path is unchanged: hover writes the pointer cursor synchronously', () => {
    vi.stubGlobal('requestAnimationFrame', (cb: () => void) => {
      cb()
      return 1
    })
    const term = {
      is_alt_screen: false,
      is_mouse_tracking: false,
      link_at: () => ({ url: 'u', kind: 1, start_col: 0, end_col: 3 })
      // no linkAtAsync / clearHover → in-process engine
    } as unknown as AtermTerminal
    const canvas = makeCanvas()
    attachAtermLinkInput({
      canvas,
      term,
      metrics: { dpr: 1, cellWidth: 10, cellHeight: 10 },
      redraw: vi.fn(),
      isDisposed: () => false,
      openUrl: vi.fn(),
      getFileLinkOpener: () => null
    })
    canvas.dispatchEvent(new MouseEvent('mousemove', { clientX: 5, clientY: 5 }))
    expect(canvas.style.cursor).toBe('pointer')
  })
})
