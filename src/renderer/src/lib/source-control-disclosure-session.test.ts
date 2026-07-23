import { afterEach, describe, expect, it } from 'vitest'
import {
  createDefaultDisclosureState,
  loadSessionDisclosureState,
  readDisclosureStateForWorktree,
  saveSessionDisclosureState,
  writeDisclosureStateForWorktree
} from './source-control-disclosure-session'

describe('source control disclosure session cache', () => {
  afterEach(() => {
    saveSessionDisclosureState({})
  })

  it('gives fresh worktrees the default disclosure state (history collapsed)', () => {
    const state = readDisclosureStateForWorktree(loadSessionDisclosureState(), 'wt-fresh')
    expect(state.collapsedSections.has('history')).toBe(true)
    expect(state.collapsedTreeDirs.size).toBe(0)
    expect(state.filterExpanded).toBe(false)
  })

  it('falls back to defaults for a null/undefined worktree id', () => {
    expect(readDisclosureStateForWorktree({}, null).collapsedSections.has('history')).toBe(true)
    expect(readDisclosureStateForWorktree({}, undefined).filterExpanded).toBe(false)
  })

  it('restores a worktree disclosure state after Source Control remounts in the same session', () => {
    // Simulate the user expanding history and collapsing a tree dir on worktree A.
    let store = loadSessionDisclosureState()
    store = writeDisclosureStateForWorktree(store, 'wt-a', {
      collapsedSections: new Set(),
      collapsedTreeDirs: new Set(['src/renderer']),
      filterExpanded: true
    })
    saveSessionDisclosureState(store)

    // Remount reads from the module cache rather than re-deriving defaults.
    const restored = readDisclosureStateForWorktree(loadSessionDisclosureState(), 'wt-a')
    expect(restored.collapsedSections.has('history')).toBe(false)
    expect(restored.collapsedTreeDirs.has('src/renderer')).toBe(true)
    expect(restored.filterExpanded).toBe(true)
  })

  it('keeps each worktree isolated so a switch does not leak state', () => {
    let store = writeDisclosureStateForWorktree({}, 'wt-a', { filterExpanded: true })
    store = writeDisclosureStateForWorktree(store, 'wt-b', {
      collapsedSections: new Set(['branch'])
    })

    const a = readDisclosureStateForWorktree(store, 'wt-a')
    const b = readDisclosureStateForWorktree(store, 'wt-b')

    expect(a.filterExpanded).toBe(true)
    // wt-a never touched its sections, so it keeps the default (history collapsed).
    expect(a.collapsedSections.has('history')).toBe(true)
    expect(a.collapsedSections.has('branch')).toBe(false)
    // wt-b's explicit section set does not carry wt-a's filter state.
    expect(b.filterExpanded).toBe(false)
    expect(b.collapsedSections.has('branch')).toBe(true)
  })

  it('writes immutably without mutating the previous store or its sets', () => {
    const original = writeDisclosureStateForWorktree({}, 'wt-a', {
      collapsedSections: new Set(['history'])
    })
    const next = writeDisclosureStateForWorktree(original, 'wt-a', {
      filterExpanded: true
    })

    expect(next).not.toBe(original)
    expect(original['wt-a'].filterExpanded).toBe(false)
    expect(next['wt-a'].filterExpanded).toBe(true)
    // Unrelated fields survive a partial update.
    expect(next['wt-a'].collapsedSections.has('history')).toBe(true)
  })

  it('exposes a default state factory that collapses only history', () => {
    const defaults = createDefaultDisclosureState()
    expect([...defaults.collapsedSections]).toEqual(['history'])
    expect(defaults.collapsedTreeDirs.size).toBe(0)
    expect(defaults.filterExpanded).toBe(false)
  })
})
