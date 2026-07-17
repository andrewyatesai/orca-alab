import type { Page } from '@stablyai/playwright-test'

export type ActiveTerminalScrollState = {
  viewportY: number
  scrollTop: number | null
}

export async function scrollActiveTerminalToBottom(page: Page): Promise<void> {
  await page.evaluate(() => {
    const pane = (() => {
      const store = window.__store
      const state = store?.getState()
      const worktreeId = state?.activeWorktreeId
      const tabId =
        state?.activeTabType === 'terminal'
          ? state.activeTabId
          : worktreeId
            ? (state?.activeTabIdByWorktree?.[worktreeId] ?? null)
            : null
      const manager = tabId ? window.__paneManagers?.get(tabId) : null
      const candidate = manager?.getActivePane?.() ?? manager?.getPanes?.()[0] ?? null
      if (!candidate) {
        throw new Error('Active terminal pane is unavailable')
      }
      return candidate
    })()
    pane.terminal.scrollToBottom()
  })
}

export async function scrollActiveTerminalViewportElement(page: Page): Promise<void> {
  await page.evaluate(() => {
    const pane = (() => {
      const store = window.__store
      const state = store?.getState()
      const worktreeId = state?.activeWorktreeId
      const tabId =
        state?.activeTabType === 'terminal'
          ? state.activeTabId
          : worktreeId
            ? (state?.activeTabIdByWorktree?.[worktreeId] ?? null)
            : null
      const manager = tabId ? window.__paneManagers?.get(tabId) : null
      const candidate = manager?.getActivePane?.() ?? manager?.getPanes?.()[0] ?? null
      if (!candidate) {
        throw new Error('Active terminal pane is unavailable')
      }
      return candidate
    })()
    const viewport = pane.container.querySelector<HTMLElement>('.xterm-viewport')
    if (!viewport) {
      throw new Error('Active terminal viewport is unavailable')
    }
    // Why: Linux CI can drop wheel delivery entirely under PTY flood; changing
    // the viewport scrollTop exercises xterm's DOM scroll synchronization.
    viewport.scrollTop = Math.max(0, viewport.scrollTop - 1200)
    viewport.dispatchEvent(new Event('scroll', { bubbles: true }))
  })
}

export async function scrollActiveTerminalByApi(page: Page): Promise<void> {
  await page.evaluate(() => {
    const pane = (() => {
      const store = window.__store
      const state = store?.getState()
      const worktreeId = state?.activeWorktreeId
      const tabId =
        state?.activeTabType === 'terminal'
          ? state.activeTabId
          : worktreeId
            ? (state?.activeTabIdByWorktree?.[worktreeId] ?? null)
            : null
      const manager = tabId ? window.__paneManagers?.get(tabId) : null
      const candidate = manager?.getActivePane?.() ?? manager?.getPanes?.()[0] ?? null
      if (!candidate) {
        throw new Error('Active terminal pane is unavailable')
      }
      return candidate
    })()
    // Why: Linux/Xvfb can lose synthetic wheel/DOM scroll events under flood;
    // xterm's public API keeps this probe about viewport responsiveness.
    const targetLine = Math.max(0, pane.terminal.buffer.active.viewportY - 20)
    pane.terminal.scrollToLine(targetLine)
  })
}

export async function dispatchActiveTerminalWheelEvent(page: Page): Promise<void> {
  await page.evaluate(() => {
    const pane = (() => {
      const store = window.__store
      const state = store?.getState()
      const worktreeId = state?.activeWorktreeId
      const tabId =
        state?.activeTabType === 'terminal'
          ? state.activeTabId
          : worktreeId
            ? (state?.activeTabIdByWorktree?.[worktreeId] ?? null)
            : null
      const manager = tabId ? window.__paneManagers?.get(tabId) : null
      const candidate = manager?.getActivePane?.() ?? manager?.getPanes?.()[0] ?? null
      if (!candidate) {
        throw new Error('Active terminal pane is unavailable')
      }
      return candidate
    })()
    // Why: CI can drop CDP wheel input while the active textarea is focused;
    // dispatching on xterm's own surfaces still exercises its user scroll path.
    const wheelTargets = [
      pane.container.querySelector<HTMLElement>('.xterm'),
      pane.container.querySelector<HTMLElement>('.xterm-viewport'),
      pane.container.querySelector<HTMLElement>('.xterm-screen')
    ].filter((target): target is HTMLElement => Boolean(target))
    if (wheelTargets.length === 0) {
      throw new Error('Active terminal wheel target is unavailable')
    }
    for (const wheelTarget of wheelTargets) {
      wheelTarget.dispatchEvent(
        new WheelEvent('wheel', {
          bubbles: true,
          cancelable: true,
          deltaMode: WheelEvent.DOM_DELTA_PIXEL,
          deltaY: -1200
        })
      )
    }
  })
}

// Why: Linux/Xvfb can drop CDP wheel delivery, and tall wrapped tables need
// more scroll steps than a mouse-wheel loop can reliably deliver under CI load.
export async function scrollActiveTerminalToText(page: Page, text: string): Promise<void> {
  await page.evaluate((searchText) => {
    const pane = (() => {
      const store = window.__store
      const state = store?.getState()
      const worktreeId = state?.activeWorktreeId
      const tabId =
        state?.activeTabType === 'terminal'
          ? state.activeTabId
          : worktreeId
            ? (state?.activeTabIdByWorktree?.[worktreeId] ?? null)
            : null
      const manager = tabId ? window.__paneManagers?.get(tabId) : null
      const candidate = manager?.getActivePane?.() ?? manager?.getPanes?.()[0] ?? null
      if (!candidate) {
        throw new Error('Active terminal pane is unavailable')
      }
      return candidate
    })()
    const buffer = pane.terminal.buffer.active
    const rows = pane.terminal.rows
    // Why aterm: the engine retains off-screen scrollback but only exposes the
    // currently-visible rows by absolute index (getLine returns undefined for
    // off-screen lines, unlike xterm's full random-access buffer). So WALK the
    // scrollback with the real engine scroll — start at the top, then step down a
    // near-page at a time, scanning the visible rows at each stop — instead of
    // iterating absolute indices we can't read. All reads are live engine state.
    const absoluteRowText = (absY: number): string =>
      buffer.getLine(absY)?.translateToString(true) ?? ''
    const findVisible = (): number | null => {
      const top = buffer.viewportY
      for (let absY = top + rows - 1; absY >= top; absY -= 1) {
        if (absoluteRowText(absY).includes(searchText)) {
          return absY
        }
      }
      return null
    }
    pane.terminal.scrollToTop()
    let targetLine = findVisible()
    // Step down a near-full page (overlap one row so a match split across the page
    // boundary isn't skipped) until found or the viewport reaches the live bottom.
    const step = Math.max(1, rows - 1)
    let guard = 0
    const maxSteps = buffer.length + rows // hard bound on scrollback height
    while (targetLine === null && buffer.viewportY < buffer.baseY && guard < maxSteps) {
      pane.terminal.scrollLines(step)
      targetLine = findVisible()
      guard += 1
    }
    if (targetLine === null && buffer.viewportY >= buffer.baseY) {
      // Make sure the very bottom page (the live viewport) was scanned too.
      targetLine = findVisible()
    }
    if (targetLine === null) {
      throw new Error(`Text not found in terminal buffer: ${searchText}`)
    }
    // Why: center the target for the subsequent DOM-based visual assertion. The
    // engine places an ABSOLUTE line at/near the top visible row, so subtract half
    // a screen to center it (clamped to the oldest retained line).
    const centeredLine = Math.max(0, targetLine - Math.floor(rows / 2))
    pane.terminal.scrollToLine(centeredLine)
    // Why: the engine scroll above bypasses every production intent seam (a real
    // wheel/scrollbar/keyboard scroll marks the scrolled-off-bottom viewport as
    // pinned user intent). Without that pin a later visibility/wake enforce snaps
    // the viewport back to the live bottom before the golden reads geometry. So
    // reproduce a real upward wheel: dispatch a 'wheel' on the same host element the
    // scroll-intent tracker listens on (capture) — its onWheel records the pinned
    // viewport through the ACTUAL production path (markTerminalPinnedViewport).
    // Dispatching on the host (not the canvas) never reaches aterm's own scroll
    // handler, so the centered position is preserved (no double-scroll), yet the
    // intent-tracking capture listener still fires.
    let intentWheelDelivered = false
    const confirmIntentWheel = (): void => {
      intentWheelDelivered = true
    }
    pane.container.addEventListener('wheel', confirmIntentWheel, { capture: true })
    pane.container.dispatchEvent(
      new WheelEvent('wheel', { bubbles: true, cancelable: true, deltaY: -120 })
    )
    pane.container.removeEventListener('wheel', confirmIntentWheel, { capture: true })
    // Fail loud (not flaky) if the tracker's capture 'wheel' listener never runs —
    // that would mean the pin was silently skipped and the restore would flake.
    if (!intentWheelDelivered) {
      throw new Error('scroll-intent wheel listener did not receive the pinning wheel event')
    }
    pane.terminal.focus()
  }, text)
}

export async function readActiveTerminalScrollState(
  page: Page
): Promise<ActiveTerminalScrollState> {
  return page.evaluate(() => {
    const pane = (() => {
      const store = window.__store
      const state = store?.getState()
      const worktreeId = state?.activeWorktreeId
      const tabId =
        state?.activeTabType === 'terminal'
          ? state.activeTabId
          : worktreeId
            ? (state?.activeTabIdByWorktree?.[worktreeId] ?? null)
            : null
      const manager = tabId ? window.__paneManagers?.get(tabId) : null
      const candidate = manager?.getActivePane?.() ?? manager?.getPanes?.()[0] ?? null
      if (!candidate) {
        throw new Error('Active terminal pane is unavailable')
      }
      return candidate
    })()
    const viewport = pane.container.querySelector<HTMLElement>('.xterm-viewport')
    return {
      viewportY: pane.terminal.buffer.active.viewportY,
      scrollTop: viewport?.scrollTop ?? null
    }
  })
}
