import { describe, expect, it } from 'vitest'
import { formatSnapshot, formatTabListWithProfiles, formatTabShow } from './browser-format'
import { formatGetAppState, formatListApps, formatListWindows } from './computer-format'
import { sanitizeUntrustedTerminalText } from './terminal-safe-text'

const ESC = String.fromCharCode(0x1b)
const BEL = String.fromCharCode(0x07)
const DEL = String.fromCharCode(0x7f)
const C1_CSI = String.fromCharCode(0x9b)
const C1_OSC = String.fromCharCode(0x9d)
const CSI_RED = `${ESC}[31m`
const OSC8_LINK = `${ESC}]8;;https://evil.example${ESC}\\`
const OSC52_CLIPBOARD = `${ESC}]52;c;ZXZpbA==${BEL}`

// Structural newlines are the formatter's own line separators; a field-injected newline is
// stripped by the sanitizer, so checking each line (tab still allowed) validates that no
// control introducer survived inside an interpolated field.
function hasControlCharsInAnyLine(value: string): boolean {
  for (const line of value.split('\n')) {
    for (const char of line) {
      const code = char.codePointAt(0) ?? 0
      if (code !== 0x09 && (code <= 0x1f || code === 0x7f || (code >= 0x80 && code <= 0x9f))) {
        return true
      }
    }
  }
  return false
}

describe('sanitizeUntrustedTerminalText', () => {
  it('strips ESC/CSI/OSC control introducers but keeps the inert printable remainder', () => {
    const sanitized = sanitizeUntrustedTerminalText(`hi${CSI_RED}there`)
    expect(hasControlCharsInAnyLine(sanitized)).toBe(false)
    expect(sanitized).toBe('hi[31mthere')
  })

  it('removes OSC-8 hyperlink and OSC-52 clipboard sequences', () => {
    expect(hasControlCharsInAnyLine(sanitizeUntrustedTerminalText(OSC8_LINK))).toBe(false)
    expect(hasControlCharsInAnyLine(sanitizeUntrustedTerminalText(OSC52_CLIPBOARD))).toBe(false)
  })

  it('strips C0 controls including CR/LF and DEL, plus C1 introducers', () => {
    const sanitized = sanitizeUntrustedTerminalText(`a\r\nb${DEL}c${C1_CSI}${C1_OSC}d`)
    expect(sanitized).toBe('abcd')
    expect(hasControlCharsInAnyLine(sanitized)).toBe(false)
  })

  it('keeps tab and ordinary printable/unicode text', () => {
    expect(sanitizeUntrustedTerminalText('a\tb — café 🐳')).toBe('a\tb — café 🐳')
  })
})

describe('browser formatters neutralize attacker-controlled titles and urls', () => {
  it('formatTabListWithProfiles strips escapes from tab title and url', () => {
    const output = formatTabListWithProfiles(
      {
        tabs: [
          {
            index: 0,
            browserPageId: 'page-1',
            title: `Evil${CSI_RED}Title`,
            url: `https://x${OSC52_CLIPBOARD}`,
            active: true
          }
        ]
      } as never,
      false
    )
    expect(hasControlCharsInAnyLine(output)).toBe(false)
  })

  it('formatSnapshot strips escapes from page title and url', () => {
    const output = formatSnapshot({
      browserPageId: 'page-1',
      title: `Evil${OSC8_LINK}`,
      url: `https://x${CSI_RED}`,
      snapshot: 'tree'
    } as never)
    // Snapshot body itself is not sanitized; only assert the header fields are clean.
    const header = output.split('\ntree')[0]
    expect(hasControlCharsInAnyLine(header)).toBe(false)
  })

  it('formatTabShow strips escapes from title and url', () => {
    const output = formatTabShow({
      tab: {
        browserPageId: 'page-1',
        title: `Evil${CSI_RED}`,
        url: `https://x${OSC52_CLIPBOARD}`,
        active: false
      }
    } as never)
    expect(hasControlCharsInAnyLine(output)).toBe(false)
  })
})

describe('computer formatters neutralize attacker-controlled window and app names', () => {
  it('formatListWindows strips escapes from window title', () => {
    const output = formatListWindows({
      app: { name: 'Browser' },
      windows: [
        {
          index: 0,
          id: 1,
          title: `Inbox${OSC52_CLIPBOARD}`,
          width: 800,
          height: 600
        }
      ]
    } as never)
    expect(hasControlCharsInAnyLine(output)).toBe(false)
  })

  it('formatListWindows strips escapes from app name on the no-windows path', () => {
    const output = formatListWindows({
      app: { name: `App${CSI_RED}` },
      windows: []
    } as never)
    expect(hasControlCharsInAnyLine(output)).toBe(false)
  })

  it('formatListApps strips escapes from app name and bundle id', () => {
    const output = formatListApps({
      apps: [{ name: `App${CSI_RED}`, bundleId: `com.evil${C1_CSI}`, pid: 10 }]
    } as never)
    expect(hasControlCharsInAnyLine(output)).toBe(false)
  })

  it('formatGetAppState strips escapes from app name, window title, and follow-up command', () => {
    const output = formatGetAppState({
      snapshot: {
        id: 'snap',
        app: { name: `Editor${CSI_RED}`, bundleId: null, pid: 1 },
        window: { title: `Doc${OSC8_LINK}`, id: null, index: 0, width: 1, height: 1 },
        coordinateSpace: 'window',
        treeText: 'tree',
        elementCount: 0,
        focusedElementId: null,
        truncation: { truncated: false }
      },
      screenshot: null,
      screenshotStatus: { state: 'skipped', reason: 'no_screenshot_flag' }
    } as never)
    // treeText is intended output; assert the formatted header lines are clean.
    const header = output.split('\ntree')[0]
    expect(hasControlCharsInAnyLine(header)).toBe(false)
  })
})
