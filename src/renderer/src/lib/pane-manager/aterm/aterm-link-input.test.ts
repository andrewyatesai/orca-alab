/**
 * @vitest-environment happy-dom
 */
import { describe, expect, it, vi } from 'vitest'
import { attachAtermLinkInput, type AtermLinkInput } from './aterm-link-input'
import type { AtermLinkTooltipHover } from './aterm-link-tooltip'
import type { AtermTerminal } from './aterm_wasm.js'
import type { ILink, ILinkProvider } from './terminal-types'

// Cells are 10×20 device px at dpr 1, so clientX/Y map 1:1 to device px.
const CELL_W = 10
const CELL_H = 20

function fakeTerm(): AtermTerminal {
  return {
    is_alt_screen: false,
    is_mouse_tracking: false,
    display_origin_absolute: 0,
    link_at: () => undefined // the engine reports NO link — providers own these cells
  } as unknown as AtermTerminal
}

function providerWithLink(link: ILink): ILinkProvider {
  return {
    provideLinks: (_bufferLineNumber, callback) => callback([link])
  }
}

function makeLink(overrides: Partial<ILink> = {}): ILink {
  return {
    // 1-based inclusive columns 1..6 on the first buffer line.
    range: { start: { x: 1, y: 1 }, end: { x: 6, y: 1 } },
    text: 'term_abc',
    activate: vi.fn(),
    hover: vi.fn(),
    leave: vi.fn(),
    ...overrides
  }
}

function tooltipSpy(): {
  hoverLink: ReturnType<typeof vi.fn<(hover: AtermLinkTooltipHover) => void>>
  leave: ReturnType<typeof vi.fn<() => void>>
} {
  return { hoverLink: vi.fn<(hover: AtermLinkTooltipHover) => void>(), leave: vi.fn<() => void>() }
}

function mount(
  link: ILink,
  linkTooltip?: ReturnType<typeof tooltipSpy>
): {
  canvas: HTMLCanvasElement
  input: AtermLinkInput
  redraw: ReturnType<typeof vi.fn>
} {
  const canvas = document.createElement('canvas')
  document.body.appendChild(canvas)
  const redraw = vi.fn()
  const input = attachAtermLinkInput({
    canvas,
    term: fakeTerm(),
    metrics: { dpr: 1, cellWidth: CELL_W, cellHeight: CELL_H },
    redraw,
    isDisposed: () => false,
    openUrl: vi.fn(),
    openOscUrl: vi.fn(),
    getFileLinkOpener: () => null,
    getLinkProviders: () => [providerWithLink(link)],
    linkTooltip
  })
  return { canvas, input, redraw }
}

function mouseEventAtCell(
  type: string,
  col: number,
  row: number,
  init: MouseEventInit = {}
): MouseEvent {
  return new MouseEvent(type, {
    bubbles: true,
    cancelable: true,
    clientX: col * CELL_W + 2,
    clientY: row * CELL_H + 2,
    button: 0,
    ...init
  })
}

// The hover path is rAF-throttled and the provider resolution is async; settle both.
async function settleHover(): Promise<void> {
  await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)))
  await Promise.resolve()
  await Promise.resolve()
  await Promise.resolve()
}

describe('attachAtermLinkInput provider links', () => {
  it('hovers a provider link where the engine reports none: underline span + pointer + hover()', async () => {
    const link = makeLink()
    const { canvas, input } = mount(link)
    canvas.dispatchEvent(mouseEventAtCell('mousemove', 2, 0))
    await settleHover()
    expect(link.hover).toHaveBeenCalledTimes(1)
    expect(canvas.style.cursor).toBe('pointer')
    // 1-based inclusive [1..6] → 0-based exclusive [0, 6) on display row 0.
    expect(input.hoveredSpan()).toEqual({ row: 0, startCol: 0, endCol: 6 })
    input.dispose()
  })

  it('activates the cached provider link on modifier click and preventDefaults SYNCHRONOUSLY', async () => {
    const link = makeLink()
    const { canvas, input } = mount(link)
    canvas.dispatchEvent(mouseEventAtCell('mousemove', 2, 0))
    await settleHover()
    const click = mouseEventAtCell('click', 2, 0, { ctrlKey: true })
    canvas.dispatchEvent(click)
    // No awaiting: activation + preventDefault must land in the click task.
    expect(link.activate).toHaveBeenCalledTimes(1)
    expect(vi.mocked(link.activate).mock.calls[0][1]).toBe('term_abc')
    expect(click.defaultPrevented).toBe(true)
    input.dispose()
  })

  it('fires leave() and drops the affordance when the pointer exits the link', async () => {
    const link = makeLink()
    const { canvas, input } = mount(link)
    canvas.dispatchEvent(mouseEventAtCell('mousemove', 2, 0))
    await settleHover()
    expect(input.hoveredSpan()).not.toBeNull()
    // Column 8 is outside the [1..6] range → the provider yields no hit there.
    canvas.dispatchEvent(mouseEventAtCell('mousemove', 8, 0))
    await settleHover()
    expect(link.leave).toHaveBeenCalled()
    expect(input.hoveredSpan()).toBeNull()
    expect(canvas.style.cursor).toBe('')
    input.dispose()
  })

  it('does not activate without the platform modifier (plain click stays a selection)', async () => {
    const link = makeLink()
    const { canvas, input } = mount(link)
    canvas.dispatchEvent(mouseEventAtCell('mousemove', 2, 0))
    await settleHover()
    canvas.dispatchEvent(mouseEventAtCell('click', 2, 0))
    expect(link.activate).not.toHaveBeenCalled()
    input.dispose()
  })
})

describe('attachAtermLinkInput engine link kinds (#6880)', () => {
  function mountEngineLink(hit: { url: string; kind: number }): {
    canvas: HTMLCanvasElement
    input: AtermLinkInput
    openUrl: ReturnType<typeof vi.fn>
    openOscUrl: ReturnType<typeof vi.fn>
  } {
    const canvas = document.createElement('canvas')
    document.body.appendChild(canvas)
    const term = {
      is_alt_screen: false,
      is_mouse_tracking: false,
      display_origin_absolute: 0,
      link_at: () => ({ start_col: 0, end_col: 6, ...hit })
    } as unknown as AtermTerminal
    const openUrl = vi.fn()
    const openOscUrl = vi.fn()
    const input = attachAtermLinkInput({
      canvas,
      term,
      metrics: { dpr: 1, cellWidth: CELL_W, cellHeight: CELL_H },
      redraw: vi.fn(),
      isDisposed: () => false,
      openUrl,
      openOscUrl,
      getFileLinkOpener: () => null
    })
    return { canvas, input, openUrl, openOscUrl }
  }

  it('kind-0 OSC 8 hit routes through the OSC opener, not the HTTP opener', () => {
    const { canvas, input, openUrl, openOscUrl } = mountEngineLink({
      url: 'file:///tmp/report.txt',
      kind: 0
    })
    const click = mouseEventAtCell('click', 2, 0, { ctrlKey: true })
    canvas.dispatchEvent(click)
    expect(openOscUrl).toHaveBeenCalledTimes(1)
    expect(openOscUrl.mock.calls[0][0]).toBe('file:///tmp/report.txt')
    // The raw MouseEvent travels with the hit (the scheme router reads Shift/modifiers).
    expect(openOscUrl.mock.calls[0][1]).toBe(click)
    expect(openUrl).not.toHaveBeenCalled()
    expect(click.defaultPrevented).toBe(true)
    input.dispose()
  })

  it('kind-1 URL hit still uses the HTTP opener (Shift → system browser)', () => {
    const { canvas, input, openUrl, openOscUrl } = mountEngineLink({
      url: 'https://example.test/',
      kind: 1
    })
    canvas.dispatchEvent(mouseEventAtCell('click', 2, 0, { ctrlKey: true, shiftKey: true }))
    expect(openUrl).toHaveBeenCalledWith('https://example.test/', { forceSystemBrowser: true })
    expect(openOscUrl).not.toHaveBeenCalled()
    input.dispose()
  })

  it('worker async click path routes kind 0 through the OSC opener too', async () => {
    const canvas = document.createElement('canvas')
    document.body.appendChild(canvas)
    const term = {
      is_alt_screen: false,
      is_mouse_tracking: false,
      display_origin_absolute: 0,
      link_at: () => undefined, // lagging sync snapshot has no hit
      linkAtAsync: () =>
        Promise.resolve({ url: 'file:///srv/log.txt', kind: 0, start_col: 0, end_col: 6 }),
      clearHover: vi.fn()
    } as unknown as AtermTerminal
    const openUrl = vi.fn()
    const openOscUrl = vi.fn()
    const input = attachAtermLinkInput({
      canvas,
      term,
      metrics: { dpr: 1, cellWidth: CELL_W, cellHeight: CELL_H },
      redraw: vi.fn(),
      isDisposed: () => false,
      openUrl,
      openOscUrl,
      getFileLinkOpener: () => null
    })
    canvas.dispatchEvent(mouseEventAtCell('click', 2, 0, { ctrlKey: true }))
    await Promise.resolve()
    await Promise.resolve()
    expect(openOscUrl).toHaveBeenCalledTimes(1)
    expect(openOscUrl.mock.calls[0][0]).toBe('file:///srv/log.txt')
    expect(openUrl).not.toHaveBeenCalled()
    input.dispose()
  })
})

describe('attachAtermLinkInput tooltip notifications', () => {
  it('feeds a resolved provider hover to the tooltip sink with its span + text', async () => {
    const link = makeLink()
    const linkTooltip = tooltipSpy()
    const { canvas, input } = mount(link, linkTooltip)
    canvas.dispatchEvent(mouseEventAtCell('mousemove', 2, 0))
    await settleHover()
    expect(linkTooltip.hoverLink).toHaveBeenCalledWith({
      span: { row: 0, startCol: 0, endCol: 6 },
      text: 'term_abc',
      kind: 'provider'
    })
    input.dispose()
  })

  it('tells the tooltip to leave when the pointer exits the link and on dispose', async () => {
    const link = makeLink()
    const linkTooltip = tooltipSpy()
    const { canvas, input } = mount(link, linkTooltip)
    canvas.dispatchEvent(mouseEventAtCell('mousemove', 2, 0))
    await settleHover()
    canvas.dispatchEvent(mouseEventAtCell('mousemove', 8, 0))
    await settleHover()
    expect(linkTooltip.leave).toHaveBeenCalled()
    const leaveCalls = linkTooltip.leave.mock.calls.length
    input.dispose()
    expect(linkTooltip.leave.mock.calls.length).toBeGreaterThan(leaveCalls)
  })
})

describe('attachAtermLinkInput context-menu targets (#9279)', () => {
  function mountForContext(overrides: {
    term?: Partial<Record<string, unknown>>
    getFileLinkOpener?: () => ((raw: string, sys: boolean) => void) | null
    getLinkProviders?: () => ILinkProvider[]
  }): {
    input: AtermLinkInput
    openUrl: ReturnType<typeof vi.fn>
    openOscUrl: ReturnType<typeof vi.fn>
  } {
    const canvas = document.createElement('canvas')
    document.body.appendChild(canvas)
    const term = {
      is_alt_screen: false,
      is_mouse_tracking: false,
      display_origin_absolute: 0,
      link_at: () => undefined,
      ...overrides.term
    } as unknown as AtermTerminal
    const openUrl = vi.fn()
    const openOscUrl = vi.fn()
    const input = attachAtermLinkInput({
      canvas,
      term,
      metrics: { dpr: 1, cellWidth: CELL_W, cellHeight: CELL_H },
      redraw: vi.fn(),
      isDisposed: () => false,
      openUrl,
      openOscUrl,
      getFileLinkOpener: overrides.getFileLinkOpener ?? (() => null),
      getLinkProviders: overrides.getLinkProviders
    })
    return { input, openUrl, openOscUrl }
  }

  it('contextLinkTargetAt returns the engine hit at the right-clicked cell', async () => {
    const linkAt = vi.fn((row: number, col: number) =>
      row === 1 && col === 3
        ? { url: 'https://example.test/', kind: 1, start_col: 0, end_col: 6 }
        : undefined
    )
    const { input } = mountForContext({ term: { link_at: linkAt } })
    // Cell (col 3, row 1) center in client px.
    const target = await input.contextLinkTargetAt(3 * CELL_W + 2, 1 * CELL_H + 2)
    expect(target).toEqual({ kind: 'url', url: 'https://example.test/' })
    // A miss at another cell resolves null, not a stale target.
    expect(await input.contextLinkTargetAt(8 * CELL_W + 2, 0)).toBeNull()
    input.dispose()
  })

  it('maps engine kinds: osc8 and file targets; kind-3 "other" yields no target', async () => {
    const mk = (kind: number) =>
      mountForContext({
        term: { link_at: () => ({ url: 'raw-span', kind, start_col: 0, end_col: 6 }) }
      })
    const osc = mk(0)
    expect(await osc.input.contextLinkTargetAt(2, 2)).toEqual({ kind: 'osc8', url: 'raw-span' })
    const file = mk(2)
    expect(await file.input.contextLinkTargetAt(2, 2)).toEqual({
      kind: 'file',
      rawPathText: 'raw-span'
    })
    const other = mk(3)
    expect(await other.input.contextLinkTargetAt(2, 2)).toBeNull()
    osc.input.dispose()
    file.input.dispose()
    other.input.dispose()
  })

  it('resolves a FRESH worker hit via linkAtAsync when the facade exposes it', async () => {
    const { input } = mountForContext({
      term: {
        link_at: () => undefined, // lagging sync snapshot has no hit
        linkAtAsync: () =>
          Promise.resolve({ url: 'file:///srv/log.txt', kind: 0, start_col: 0, end_col: 6 })
      }
    })
    expect(await input.contextLinkTargetAt(2, 2)).toEqual({
      kind: 'osc8',
      url: 'file:///srv/log.txt'
    })
    input.dispose()
  })

  it('falls back to provider links when the engine reports none', async () => {
    const link = makeLink()
    const { input } = mountForContext({ getLinkProviders: () => [providerWithLink(link)] })
    const target = await input.contextLinkTargetAt(2 * CELL_W + 2, 2)
    expect(target?.kind).toBe('provider')
    if (target?.kind !== 'provider') {
      return
    }
    expect(target.text).toBe('term_abc')
    target.activate(new MouseEvent('click'))
    expect(link.activate).toHaveBeenCalledTimes(1)
    expect(vi.mocked(link.activate).mock.calls[0][1]).toBe('term_abc')
    input.dispose()
  })

  it('returns null on alt-screen and under mouse tracking', async () => {
    const altScreen = mountForContext({
      term: {
        is_alt_screen: true,
        link_at: () => ({ url: 'https://x/', kind: 1, start_col: 0, end_col: 3 })
      }
    })
    expect(await altScreen.input.contextLinkTargetAt(2, 2)).toBeNull()
    const tracking = mountForContext({
      term: {
        is_mouse_tracking: true,
        link_at: () => ({ url: 'https://x/', kind: 1, start_col: 0, end_col: 3 })
      }
    })
    expect(await tracking.input.contextLinkTargetAt(2, 2)).toBeNull()
    altScreen.input.dispose()
    tracking.input.dispose()
  })

  it('openContextTarget routes each kind through the click path openers', () => {
    const fileOpener = vi.fn()
    const { input, openUrl, openOscUrl } = mountForContext({
      getFileLinkOpener: () => fileOpener
    })

    input.openContextTarget({ kind: 'url', url: 'https://a/' }, { openWithSystemDefault: false })
    expect(openUrl).toHaveBeenCalledWith('https://a/', { forceSystemBrowser: false })

    input.openContextTarget({ kind: 'osc8', url: 'file:///b' }, { openWithSystemDefault: true })
    expect(openOscUrl).toHaveBeenCalledTimes(1)
    expect(openOscUrl.mock.calls[0][0]).toBe('file:///b')
    // The scheme router reads Shift as the system-default hatch off the event.
    expect((openOscUrl.mock.calls[0][1] as MouseEvent).shiftKey).toBe(true)

    input.openContextTarget(
      { kind: 'file', rawPathText: 'src/app.ts:12' },
      { openWithSystemDefault: false }
    )
    expect(fileOpener).toHaveBeenCalledWith('src/app.ts:12', false)

    const activate = vi.fn()
    input.openContextTarget({ kind: 'provider', text: 'term_x', activate }, {
      openWithSystemDefault: false
    })
    expect(activate).toHaveBeenCalledTimes(1)
    // Provider activates re-check the platform modifier; the synthesized event must carry it.
    const event = activate.mock.calls[0][0] as MouseEvent
    expect(event.metaKey || event.ctrlKey).toBe(true)
    input.dispose()
  })
})

describe('attachAtermLinkInput resetHoverCache', () => {
  it('re-evaluates the SAME cell after a reset (reveal recovery), and only then', async () => {
    const canvas = document.createElement('canvas')
    document.body.appendChild(canvas)
    const linkAt = vi.fn(() => undefined)
    const term = {
      is_alt_screen: false,
      is_mouse_tracking: false,
      display_origin_absolute: 0,
      link_at: linkAt
    } as unknown as AtermTerminal
    const input = attachAtermLinkInput({
      canvas,
      term,
      metrics: { dpr: 1, cellWidth: CELL_W, cellHeight: CELL_H },
      redraw: vi.fn(),
      isDisposed: () => false,
      openUrl: vi.fn(),
      openOscUrl: vi.fn(),
      getFileLinkOpener: () => null
    })

    canvas.dispatchEvent(mouseEventAtCell('mousemove', 2, 0))
    await settleHover()
    expect(linkAt).toHaveBeenCalledTimes(1)

    // Same cell again: the short-circuit skips re-evaluation.
    canvas.dispatchEvent(mouseEventAtCell('mousemove', 2, 0))
    await settleHover()
    expect(linkAt).toHaveBeenCalledTimes(1)

    // After a reveal-recovery reset the same cell is re-evaluated.
    input.resetHoverCache()
    canvas.dispatchEvent(mouseEventAtCell('mousemove', 2, 0))
    await settleHover()
    expect(linkAt).toHaveBeenCalledTimes(2)
    input.dispose()
  })
})
