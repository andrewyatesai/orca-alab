import { describe, expect, it } from 'vitest'
import {
  MAX_TERMINAL_LINE_HEIGHT,
  MIN_TERMINAL_LINE_HEIGHT,
  normalizeTerminalLineHeight
} from './terminal-line-height-settings'

describe('normalizeTerminalLineHeight', () => {
  it.each([
    [undefined, MIN_TERMINAL_LINE_HEIGHT],
    [Number.NaN, MIN_TERMINAL_LINE_HEIGHT],
    [Number.POSITIVE_INFINITY, MIN_TERMINAL_LINE_HEIGHT],
    [0.85, MIN_TERMINAL_LINE_HEIGHT],
    [1, 1],
    [1.35, 1.35],
    [3, 3],
    // Accessibility range above the historical 3x cap (upstream #7934).
    [4.5, 4.5],
    [10, 10],
    [12, MAX_TERMINAL_LINE_HEIGHT]
  ])('normalizes %s to %s', (input, expected) => {
    expect(normalizeTerminalLineHeight(input)).toBe(expected)
  })

  it('keeps the accessibility ceiling at 10x (upstream #7934)', () => {
    expect(MAX_TERMINAL_LINE_HEIGHT).toBe(10)
  })
})
