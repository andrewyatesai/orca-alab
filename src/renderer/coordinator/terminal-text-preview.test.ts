import { describe, expect, it } from 'vitest'
import { appendBoundedTail, terminalPlainTextTail } from './terminal-text-preview'

describe('terminalPlainTextTail', () => {
  it('strips CSI colors and OSC titles', () => {
    const raw = '\u001b]0;my-title\u0007\u001b[32mgreen\u001b[0m plain'
    expect(terminalPlainTextTail(raw, 5)).toEqual(['green plain'])
  })

  it('keeps only the last maxLines and drops trailing blanks', () => {
    const raw = 'one\ntwo\nthree\nfour\n\n\n'
    expect(terminalPlainTextTail(raw, 2)).toEqual(['three', 'four'])
  })

  it('resolves CR overwrites like a progress bar', () => {
    expect(terminalPlainTextTail('downloading 10%\rdownloading 99%\n', 5)).toEqual([
      'downloading 99%'
    ])
  })

  it('keeps the longer leftover when a CR rewrite is shorter', () => {
    expect(terminalPlainTextTail('1234567890\rab\n', 5)).toEqual(['ab34567890'])
  })

  it('drops residual C0 control bytes but keeps tabs', () => {
    expect(terminalPlainTextTail('a\u0008b\tc\u0000\n', 5)).toEqual(['ab\tc'])
  })
})

describe('appendBoundedTail', () => {
  it('appends under the cap and truncates from the front over it', () => {
    expect(appendBoundedTail('abc', 'def', 10)).toBe('abcdef')
    expect(appendBoundedTail('abcdefgh', 'ijkl', 10)).toBe('cdefghijkl')
  })
})
