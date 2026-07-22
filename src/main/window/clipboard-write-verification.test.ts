import { describe, expect, it, vi } from 'vitest'
import { writeClipboardTextVerified } from './clipboard-write-verification'

// A fake OS clipboard whose read-back behavior each test scripts: `landing`
// controls which write attempts actually reach the clipboard.
function makeClipboard(opts: { landing: (attempt: number) => boolean }) {
  let stored = '<untouched>'
  let attempts = 0
  const write = vi.fn((text: string) => {
    attempts += 1
    if (opts.landing(attempts)) {
      stored = text
    }
  })
  const read = vi.fn(() => stored)
  return { write, read }
}

const immediateDelay = vi.fn(() => Promise.resolve())

describe('writeClipboardTextVerified', () => {
  it('verifies a landed write by read-back without retrying', async () => {
    const clipboard = makeClipboard({ landing: () => true })
    const ok = await writeClipboardTextVerified('hello', 'clipboard', {
      ...clipboard,
      delay: immediateDelay
    })
    expect(ok).toBe(true)
    expect(clipboard.write).toHaveBeenCalledTimes(1)
    expect(clipboard.read).toHaveBeenCalledTimes(1)
  })

  it('retries once on a transient mismatch (Win32 open-clipboard contention)', async () => {
    // The first write is swallowed; the retry lands.
    const clipboard = makeClipboard({ landing: (attempt) => attempt === 2 })
    const delay = vi.fn(() => Promise.resolve())
    const ok = await writeClipboardTextVerified('hello', 'clipboard', { ...clipboard, delay })
    expect(ok).toBe(true)
    expect(clipboard.write).toHaveBeenCalledTimes(2)
    expect(delay).toHaveBeenCalledWith(150)
  })

  it('returns false and logs a structured "unverified" warning when the clipboard stays unchanged', async () => {
    const clipboard = makeClipboard({ landing: () => false })
    const warn = vi.fn()
    const ok = await writeClipboardTextVerified('hello', 'selection', {
      ...clipboard,
      delay: immediateDelay,
      warn
    })
    expect(ok).toBe(false)
    expect(clipboard.write).toHaveBeenCalledTimes(2)
    // Wording matters: a clipboard manager may have rewritten contents between
    // write and read-back, so this is "could not be verified", not "failed".
    expect(warn).toHaveBeenCalledTimes(1)
    expect(warn.mock.calls[0][0]).toContain('could not be verified')
    expect(warn.mock.calls[0][1]).toMatchObject({ target: 'selection', length: 5 })
  })

  it('bounds the comparison for large payloads (length + head/tail, one read per attempt)', async () => {
    const big = 'a'.repeat(300 * 1024)
    // A middle-corrupted read-back with identical length and edges: the bounded
    // compare accepts it (full compare would not) — proving the bound is real.
    const corrupted = `${big.slice(0, 8 * 1024)}X${big.slice(8 * 1024 + 1)}`
    const read = vi.fn(() => corrupted)
    const write = vi.fn()
    const ok = await writeClipboardTextVerified(big, 'clipboard', {
      write,
      read,
      delay: immediateDelay
    })
    expect(ok).toBe(true)
    expect(read).toHaveBeenCalledTimes(1)

    // A large payload with a differing TAIL still fails the bounded compare.
    const tailBroken = `${big.slice(0, -1)}X`
    const failing = await writeClipboardTextVerified(big, 'clipboard', {
      write: vi.fn(),
      read: vi.fn(() => tailBroken),
      delay: immediateDelay,
      warn: vi.fn()
    })
    expect(failing).toBe(false)
  })
})
