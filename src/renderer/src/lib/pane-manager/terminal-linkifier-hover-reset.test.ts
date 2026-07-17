import { describe, expect, it, vi } from 'vitest'
import type { AtermTerminalFacade } from './aterm/aterm-terminal-facade'
import { resetTerminalLinkifierHoverState } from './terminal-linkifier-hover-reset'

// The behavioral contract (same-cell mousemove re-evaluates after a reset)
// is proven against the real link input in aterm-link-input.test.ts; this
// covers the reveal-path seam onto the facade.
describe('resetTerminalLinkifierHoverState', () => {
  it('invalidates the facade link hover cell cache', () => {
    const resetLinkHoverCache = vi.fn()
    resetTerminalLinkifierHoverState({ resetLinkHoverCache } as unknown as AtermTerminalFacade)

    expect(resetLinkHoverCache).toHaveBeenCalledTimes(1)
  })
})
