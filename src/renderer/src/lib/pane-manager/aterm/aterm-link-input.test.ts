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
