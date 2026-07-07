import { readFileSync } from 'node:fs'
import { beforeAll, describe, expect, it } from 'vitest'
import { computeLineStats, initGitWasmForTestFromBytes, isGitWasmReady } from './git-line-stats'

// Captured at import time, BEFORE the beforeAll init below runs: pins the
// null-until-ready contract (consumers fall back to numstat counts).
const preInitReady = isGitWasmReady()
const preInitResult = computeLineStats('a\nb', 'a\nc', 'modified')

beforeAll(() => {
  // vitest runs under Node (no Chromium sync-compile restriction), so init the
  // wasm synchronously from the committed renderer-tree bytes.
  initGitWasmForTestFromBytes(readFileSync(new URL('./orca_git_wasm_bg.wasm', import.meta.url)))
})

describe('computeLineStats (orca-git wasm)', () => {
  it('returns null before the wasm is initialised (numstat fallback contract)', () => {
    expect(preInitReady).toBe(false)
    expect(preInitResult).toBeNull()
    expect(isGitWasmReady()).toBe(true)
  })

  it('keeps existing added, deleted, and modified line count behavior', () => {
    expect(computeLineStats('', 'a\nb', 'added')).toEqual({ added: 2, removed: 0 })
    expect(computeLineStats('a\nb\n', '', 'deleted')).toEqual({ added: 0, removed: 3 })
    expect(computeLineStats('a\nb\nc', 'a\nc\nd', 'modified')).toEqual({
      added: 1,
      removed: 1
    })
  })

  it('counts newline-heavy added and deleted pasted content', () => {
    const content = '\n'.repeat(100_000)
    expect(computeLineStats('', content, 'added')).toEqual({ added: 100_001, removed: 0 })
    expect(computeLineStats(content, '', 'deleted')).toEqual({ added: 0, removed: 100_001 })
  })

  it('matches repeated lines as a multiset, not a set', () => {
    expect(computeLineStats('same\nold\nkept', 'same\nnew\nkept', 'modified')).toEqual({
      added: 1,
      removed: 1
    })
    expect(computeLineStats('dup\ndup\ndup', 'dup', 'modified')).toEqual({
      added: 0,
      removed: 2
    })
  })

  it('keeps the existing large modified-file guard', () => {
    expect(computeLineStats('x'.repeat(250_001), 'y'.repeat(250_000), 'modified')).toBeNull()
  })
})
