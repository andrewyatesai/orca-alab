/**
 * @vitest-environment happy-dom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Wave-1 waiver pin: 1D routed this unclaimed aterm glue through its verified-copy
// seam; pin both halves of the contract so a later refactor can't drop either.
const { copyTerminalTextVerifiedMock } = vi.hoisted(() => ({
  copyTerminalTextVerifiedMock: vi.fn(() => Promise.resolve(true))
}))

vi.mock('@/components/terminal-pane/terminal-copy-outcome', () => ({
  copyTerminalTextVerified: copyTerminalTextVerifiedMock
}))

import { copyAtermSelectionToClipboard } from './aterm-clipboard-copy'

describe('copyAtermSelectionToClipboard', () => {
  beforeEach(() => {
    copyTerminalTextVerifiedMock.mockClear()
  })
  afterEach(() => {
    delete (window as unknown as { __atermLastCopied?: string }).__atermLastCopied
  })

  it('routes the copy through the verified seam as a copy-on-select outcome', () => {
    copyAtermSelectionToClipboard('drag selection')
    expect(copyTerminalTextVerifiedMock).toHaveBeenCalledWith('drag selection', 'copy-on-select')
  })

  it('still surfaces the text on __atermLastCopied for hidden-window e2e assertions', () => {
    copyAtermSelectionToClipboard('e2e probe')
    expect(
      (window as unknown as { __atermLastCopied?: string }).__atermLastCopied
    ).toBe('e2e probe')
  })
})
