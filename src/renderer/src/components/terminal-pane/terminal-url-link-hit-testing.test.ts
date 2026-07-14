import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  extractTerminalHttpLinks,
  isDesktopHttpLinkFallbackActivation,
  TERMINAL_HTTP_URL_MAX_LENGTH
} from './terminal-url-link-hit-testing'

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('isDesktopHttpLinkFallbackActivation', () => {
  function mouse(overrides: Partial<MouseEvent>): MouseEvent {
    return { defaultPrevented: false, button: 0, ...overrides } as MouseEvent
  }

  it('activates on a Cmd gesture but leaves Alt+Cmd to the child TUI on macOS', () => {
    vi.stubGlobal('navigator', { userAgent: 'Macintosh' })
    expect(isDesktopHttpLinkFallbackActivation(mouse({ metaKey: true }))).toBe(true)
    expect(isDesktopHttpLinkFallbackActivation(mouse({ metaKey: true, altKey: true }))).toBe(false)
  })

  it('activates on a Ctrl gesture but leaves Alt+Ctrl to the child TUI on other platforms', () => {
    vi.stubGlobal('navigator', { userAgent: 'Windows' })
    expect(isDesktopHttpLinkFallbackActivation(mouse({ ctrlKey: true }))).toBe(true)
    expect(isDesktopHttpLinkFallbackActivation(mouse({ ctrlKey: true, altKey: true }))).toBe(false)
  })

  it('ignores non-primary buttons and already-handled events', () => {
    vi.stubGlobal('navigator', { userAgent: 'Macintosh' })
    expect(isDesktopHttpLinkFallbackActivation(mouse({ metaKey: true, button: 1 }))).toBe(false)
    expect(
      isDesktopHttpLinkFallbackActivation(mouse({ metaKey: true, defaultPrevented: true }))
    ).toBe(false)
  })
})

describe('extractTerminalHttpLinks', () => {
  it('extracts regular http links and trims terminal trailing punctuation', () => {
    const line = 'open https://example.com/path?x=1.'

    expect(extractTerminalHttpLinks(line)).toEqual([
      {
        url: 'https://example.com/path?x=1',
        startIndex: 'open '.length,
        endIndex: line.length - 1
      }
    ])
  })

  it('requires a word boundary before the http scheme', () => {
    expect(extractTerminalHttpLinks('prefixhttps://example.com/path')).toEqual([])
    expect(extractTerminalHttpLinks('prefix https://example.com/path')).toHaveLength(1)
  })

  it('rejects overlong pasted URL candidates before URL parsing', () => {
    const overlongUrl = `https://example.com/${'a'.repeat(TERMINAL_HTTP_URL_MAX_LENGTH)}`

    expect(extractTerminalHttpLinks(overlongUrl)).toEqual([])
  })

  it('scans large pasted terminal lines without regex match iteration', () => {
    const matchAllSpy = vi.spyOn(String.prototype, 'matchAll')
    const pastedPrefix = 'pasted terminal noise '.repeat(10_000)
    const line = `${pastedPrefix}https://example.com/docs?q=orca.`

    expect(extractTerminalHttpLinks(line)).toEqual([
      {
        url: 'https://example.com/docs?q=orca',
        startIndex: pastedPrefix.length,
        endIndex: line.length - 1
      }
    ])
    expect(matchAllSpy).not.toHaveBeenCalled()
  })
})
