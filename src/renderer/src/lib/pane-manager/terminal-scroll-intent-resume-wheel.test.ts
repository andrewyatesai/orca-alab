import { afterEach, describe, expect, it, vi } from 'vitest'
import { attachTerminalScrollIntentTracking } from './terminal-scroll-intent-dom-tracking'
import {
  beginSuppressScrollIntentWrites,
  endSuppressScrollIntentWrites,
  enforceTerminalCurrentScrollIntent,
  getTerminalScrollIntentKind
} from './terminal-scroll-intent'

class TestElement extends EventTarget {
  readonly classList = { contains: () => false }
  closest(): null {
    return null
  }
}

function createTerminal() {
  const terminal = {
    buffer: { active: { type: 'normal' as const, viewportY: 100, baseY: 100 } },
    scrollToBottom: vi.fn(() => {
      terminal.buffer.active.viewportY = terminal.buffer.active.baseY
    }),
    scrollToLine: vi.fn((line: number) => {
      terminal.buffer.active.viewportY = line
    })
  }
  return terminal
}

describe('terminal scroll intent during visibility resume', () => {
  afterEach(() => {
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

  it('lets a settled upward wheel supersede the system-write freeze', async () => {
    vi.stubGlobal('requestAnimationFrame', () => 0)
    vi.useFakeTimers({ toFake: ['setTimeout'] })
    vi.stubGlobal('Element', TestElement)
    const terminal = createTerminal()
    const host = new TestElement() as unknown as HTMLElement
    const disposable = attachTerminalScrollIntentTracking(terminal, host)

    beginSuppressScrollIntentWrites()
    try {
      // Capture fires before the terminal's own wheel handler, so the immediate
      // user pin still reads the live bottom.
      const wheelUp = new Event('wheel') as WheelEvent
      Object.defineProperty(wheelUp, 'deltaY', { value: -10 })
      host.dispatchEvent(wheelUp)
      expect(getTerminalScrollIntentKind(terminal)).toBe('pinnedViewport')

      // The terminal then applies the wheel. Its microtask settle must record
      // this moved viewport despite the resume window's system-write freeze.
      terminal.buffer.active.viewportY = 60
      await Promise.resolve()

      // A stale resume re-anchor may disturb the engine before enforcing the
      // durable intent. The later explicit user position must win.
      terminal.buffer.active.viewportY = terminal.buffer.active.baseY
      enforceTerminalCurrentScrollIntent(terminal)
      expect(terminal.scrollToLine).toHaveBeenLastCalledWith(60)
      expect(terminal.buffer.active.viewportY).toBe(60)
    } finally {
      endSuppressScrollIntentWrites()
      disposable.dispose()
    }
  })
})
