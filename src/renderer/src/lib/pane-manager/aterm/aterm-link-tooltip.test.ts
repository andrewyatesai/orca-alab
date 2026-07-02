/**
 * @vitest-environment happy-dom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  ATERM_LINK_TOOLTIP_DELAY_MS,
  atermLinkTooltipHint,
  createAtermLinkTooltip,
  createAtermLinkTooltipTimeline,
  resolveAtermLinkTooltipLabel,
  type AtermLinkTooltipHover
} from './aterm-link-tooltip'

const SPAN = { row: 1, startCol: 2, endCol: 10 }

function hover(text = 'https://example.com', kind: AtermLinkTooltipHover['kind'] = 'url') {
  return { span: SPAN, text, kind }
}

beforeEach(() => {
  vi.useFakeTimers()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('atermLinkTooltipHint', () => {
  it('uses the mac modifier glyphs on mac and Ctrl elsewhere', () => {
    expect(atermLinkTooltipHint('url', true)).toBe('⌘+click to open or ⇧⌘+click for system browser')
    expect(atermLinkTooltipHint('url', false)).toBe(
      'Ctrl+click to open or Shift+Ctrl+click for system browser'
    )
  })

  it('picks kind-appropriate wording (file default-app hatch, generic provider)', () => {
    expect(atermLinkTooltipHint('osc8', true)).toBe(atermLinkTooltipHint('url', true))
    expect(atermLinkTooltipHint('file', true)).toBe('⌘+click to open or ⇧⌘+click for default app')
    expect(atermLinkTooltipHint('provider', false)).toBe('Ctrl+click to open')
  })
})

describe('resolveAtermLinkTooltipLabel', () => {
  it('defaults to "text (hint)" when no formatter is provided', () => {
    const label = resolveAtermLinkTooltipLabel(hover(), 'hint', undefined)
    expect(label).toEqual({ immediate: 'https://example.com (hint)', formatted: null })
  })

  it('lets a synchronous formatter result replace the label immediately', () => {
    const label = resolveAtermLinkTooltipLabel(hover(), 'hint', () => 'Vite — localhost:5173')
    expect(label.immediate).toBe('Vite — localhost:5173')
    expect(label.formatted).toBeNull()
  })

  it('keeps the default when the formatter returns null / throws', () => {
    expect(resolveAtermLinkTooltipLabel(hover(), 'hint', () => null).immediate).toBe(
      'https://example.com (hint)'
    )
    expect(
      resolveAtermLinkTooltipLabel(hover(), 'hint', () => {
        throw new Error('boom')
      }).immediate
    ).toBe('https://example.com (hint)')
  })

  it('normalizes an async formatter: value replaces, null/rejection keeps the default', async () => {
    const ok = resolveAtermLinkTooltipLabel(hover(), 'hint', () => Promise.resolve('labeled'))
    await expect(ok.formatted).resolves.toBe('labeled')
    const rejected = resolveAtermLinkTooltipLabel(hover(), 'hint', () =>
      Promise.reject(new Error('boom'))
    )
    await expect(rejected.formatted).resolves.toBeNull()
  })

  it('never invokes the formatter for file/provider links (upstream URL-only scope)', () => {
    const format = vi.fn(() => 'nope')
    resolveAtermLinkTooltipLabel(hover('/repo/file.ts', 'file'), 'hint', format)
    resolveAtermLinkTooltipLabel(hover('term_abc', 'provider'), 'hint', format)
    expect(format).not.toHaveBeenCalled()
  })
})

describe('createAtermLinkTooltipTimeline', () => {
  function makeTimeline() {
    const onShow = vi.fn()
    const onHide = vi.fn()
    const timeline = createAtermLinkTooltipTimeline<string>({ onShow, onHide })
    return { timeline, onShow, onHide }
  }

  it('shows only after the hover delay elapses', () => {
    const { timeline, onShow } = makeTimeline()
    timeline.hover('a', 'payload-a')
    vi.advanceTimersByTime(ATERM_LINK_TOOLTIP_DELAY_MS - 1)
    expect(onShow).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)
    expect(onShow).toHaveBeenCalledExactlyOnceWith('payload-a')
  })

  it('leave() before the delay cancels the pending show without an onHide', () => {
    const { timeline, onShow, onHide } = makeTimeline()
    timeline.hover('a', 'payload-a')
    timeline.leave()
    vi.advanceTimersByTime(ATERM_LINK_TOOLTIP_DELAY_MS * 2)
    expect(onShow).not.toHaveBeenCalled()
    expect(onHide).not.toHaveBeenCalled()
  })

  it('same-key re-hovers do NOT restart the delay (cell-to-cell moves along one link)', () => {
    const { timeline, onShow } = makeTimeline()
    timeline.hover('a', 'first')
    vi.advanceTimersByTime(300)
    timeline.hover('a', 'second')
    vi.advanceTimersByTime(200)
    // Fires on the ORIGINAL 500ms mark, with the latest payload.
    expect(onShow).toHaveBeenCalledExactlyOnceWith('second')
  })

  it('same-key re-hover while visible neither hides nor re-shows (no flicker)', () => {
    const { timeline, onShow, onHide } = makeTimeline()
    timeline.hover('a', 'payload-a')
    vi.advanceTimersByTime(ATERM_LINK_TOOLTIP_DELAY_MS)
    timeline.hover('a', 'payload-a')
    vi.runAllTimers()
    expect(onShow).toHaveBeenCalledTimes(1)
    expect(onHide).not.toHaveBeenCalled()
  })

  it('hovering a different link hides the visible tooltip and re-delays the new one', () => {
    const { timeline, onShow, onHide } = makeTimeline()
    timeline.hover('a', 'payload-a')
    vi.advanceTimersByTime(ATERM_LINK_TOOLTIP_DELAY_MS)
    timeline.hover('b', 'payload-b')
    expect(onHide).toHaveBeenCalledTimes(1)
    expect(onShow).toHaveBeenCalledTimes(1)
    vi.advanceTimersByTime(ATERM_LINK_TOOLTIP_DELAY_MS)
    expect(onShow).toHaveBeenLastCalledWith('payload-b')
  })

  it('leave() after showing hides; dispose() behaves like leave', () => {
    const { timeline, onHide } = makeTimeline()
    timeline.hover('a', 'payload-a')
    vi.advanceTimersByTime(ATERM_LINK_TOOLTIP_DELAY_MS)
    timeline.leave()
    expect(onHide).toHaveBeenCalledTimes(1)
    timeline.hover('b', 'payload-b')
    vi.advanceTimersByTime(ATERM_LINK_TOOLTIP_DELAY_MS)
    timeline.dispose()
    expect(onHide).toHaveBeenCalledTimes(2)
  })
})

describe('createAtermLinkTooltip (DOM overlay)', () => {
  function mount(
    formatLinkTooltip?: Parameters<typeof createAtermLinkTooltip>[0]['formatLinkTooltip']
  ) {
    const host = document.createElement('div')
    const canvas = document.createElement('canvas')
    const textarea = document.createElement('textarea')
    host.appendChild(canvas)
    document.body.appendChild(host)
    const tooltip = createAtermLinkTooltip({
      canvas,
      textarea,
      metrics: { dpr: 1, cellWidth: 10, cellHeight: 20 },
      isDisposed: () => false,
      formatLinkTooltip
    })
    const element = host.querySelector('[data-testid="aterm-link-tooltip"]') as HTMLElement
    return { host, canvas, textarea, tooltip, element }
  }

  it('shows the default "url (hint)" label near the span after the hover delay', () => {
    const { tooltip, element } = mount()
    tooltip.hoverLink(hover())
    expect(element.style.display).toBe('none')
    vi.advanceTimersByTime(ATERM_LINK_TOOLTIP_DELAY_MS)
    expect(element.style.display).not.toBe('none')
    expect(element.textContent).toContain('https://example.com')
    expect(element.textContent).toMatch(/click to open/)
    // happy-dom does no layout (host/tooltip measure 0), so the bottom-clip
    // check flips the tooltip above row 1: top = 1*20 - 0 - 2; left clamps to 0.
    expect(element.style.top).toBe('18px')
    expect(element.style.left).toBe('0px')
  })

  it('swaps in the async formatLinkTooltip label when it resolves for the same show', async () => {
    const { tooltip, element } = mount(() => Promise.resolve('Vite dev server — localhost:5173'))
    tooltip.hoverLink(hover('http://localhost:5173/'))
    await vi.advanceTimersByTimeAsync(ATERM_LINK_TOOLTIP_DELAY_MS)
    expect(element.textContent).toBe('Vite dev server — localhost:5173')
  })

  it('drops a stale async label that resolves after the tooltip hid', async () => {
    let resolveLabel: (value: string) => void = () => undefined
    const { tooltip, element } = mount(
      () => new Promise<string>((resolve) => (resolveLabel = resolve))
    )
    tooltip.hoverLink(hover('http://localhost:5173/'))
    await vi.advanceTimersByTimeAsync(ATERM_LINK_TOOLTIP_DELAY_MS)
    tooltip.leave()
    resolveLabel('stale label')
    await vi.advanceTimersByTimeAsync(0)
    expect(element.style.display).toBe('none')
    expect(element.textContent).not.toBe('stale label')
  })

  it.each(['wheel', 'mousedown', 'mouseleave'] as const)('hides on canvas %s', (type) => {
    const { tooltip, canvas, element } = mount()
    tooltip.hoverLink(hover())
    vi.advanceTimersByTime(ATERM_LINK_TOOLTIP_DELAY_MS)
    expect(element.style.display).not.toBe('none')
    canvas.dispatchEvent(new Event(type))
    expect(element.style.display).toBe('none')
  })

  it('hides on textarea keydown (typing changes what is under the pointer)', () => {
    const { tooltip, textarea, element } = mount()
    tooltip.hoverLink(hover())
    vi.advanceTimersByTime(ATERM_LINK_TOOLTIP_DELAY_MS)
    textarea.dispatchEvent(new KeyboardEvent('keydown', { key: 'a' }))
    expect(element.style.display).toBe('none')
  })

  it('dispose removes the overlay element and cancels pending shows', () => {
    const { host, tooltip, element } = mount()
    tooltip.hoverLink(hover())
    tooltip.dispose()
    vi.runAllTimers()
    expect(element.isConnected).toBe(false)
    expect(host.querySelector('[data-testid="aterm-link-tooltip"]')).toBeNull()
  })
})
