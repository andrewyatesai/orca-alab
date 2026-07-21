// @vitest-environment happy-dom

import { describe, expect, it } from 'vitest'
import {
  extractTerminalLinkFilesystemPath,
  isFilesystemPath
} from './rich-markdown-filesystem-path'

describe('isFilesystemPath', () => {
  it('recognizes Windows drive, UNC, and POSIX absolute paths', () => {
    expect(isFilesystemPath('C:\\Users\\me\\repo\\CLAUDE.md')).toBe(true)
    expect(isFilesystemPath('c:/Users/me/repo/CLAUDE.md')).toBe(true)
    expect(isFilesystemPath('\\\\server\\share\\CLAUDE.md')).toBe(true)
    expect(isFilesystemPath('/home/me/repo/CLAUDE.md')).toBe(true)
    expect(isFilesystemPath('  C:\\Users\\me\\CLAUDE.md  ')).toBe(true)
  })

  it('rejects URLs, prose, and ambiguous fragments', () => {
    expect(isFilesystemPath('https://example.com/docs')).toBe(false)
    expect(isFilesystemPath('http://CLAUDE.md')).toBe(false)
    expect(isFilesystemPath('mailto:me@example.com')).toBe(false)
    expect(isFilesystemPath('see C:\\Users\\me when ready')).toBe(false)
    expect(isFilesystemPath('CLAUDE.md')).toBe(false)
    expect(isFilesystemPath('/')).toBe(false)
    expect(isFilesystemPath('/foo')).toBe(false)
    expect(isFilesystemPath('a paragraph\nwith C:\\path')).toBe(false)
    expect(isFilesystemPath('')).toBe(false)
  })
})

describe('extractTerminalLinkFilesystemPath', () => {
  it('extracts a path from a single terminal-style anchor with a fabricated href', () => {
    const path = 'C:\\Users\\me\\repo\\CLAUDE.md'
    expect(
      extractTerminalLinkFilesystemPath(`<a href="http://CLAUDE.md">${path}</a>`)
    ).toBe(path)
  })

  it('extracts a path when the anchor text itself is a path', () => {
    const path = '/home/me/repo/CLAUDE.md'
    expect(
      extractTerminalLinkFilesystemPath(`<a href="${path}">${path}</a>`)
    ).toBe(path)
  })

  it('returns null for a genuine URL anchor', () => {
    expect(
      extractTerminalLinkFilesystemPath(
        '<a href="https://example.com/docs">https://example.com/docs</a>'
      )
    ).toBeNull()
  })

  it('returns null when the anchor is part of larger rich content', () => {
    expect(
      extractTerminalLinkFilesystemPath(
        '<p>see <a href="http://CLAUDE.md">C:\\repo\\CLAUDE.md</a> please</p>'
      )
    ).toBeNull()
  })

  it('returns null for multiple anchors or empty html', () => {
    expect(
      extractTerminalLinkFilesystemPath(
        '<a href="http://a">C:\\a</a><a href="http://b">C:\\b</a>'
      )
    ).toBeNull()
    expect(extractTerminalLinkFilesystemPath('')).toBeNull()
    expect(extractTerminalLinkFilesystemPath('<p>plain</p>')).toBeNull()
  })
})
