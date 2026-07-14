import { readFileSync } from 'node:fs'
import { beforeAll, describe, expect, it } from 'vitest'
import { getProjectGroupSubtreeIds } from './project-groups'
import { initGitWasmForTestFromBytes } from './git-line-stats'

// Subtree collection now runs THROUGH the Rust orca-config core via wasm. The
// pre-ready degrade (root alone, so removals/queries under-scope rather than
// wrongly include descendants) can only be observed here; the parity vectors
// pin the ready-state goldens.
const GROUPS = [
  { id: 'root', parentGroupId: null },
  { id: 'child', parentGroupId: 'root' },
  { id: 'grandchild', parentGroupId: 'child' },
  { id: 'sibling', parentGroupId: null }
]

const preInit = getProjectGroupSubtreeIds(GROUPS, 'root')

beforeAll(() => {
  initGitWasmForTestFromBytes(readFileSync(new URL('./orca_git_wasm_bg.wasm', import.meta.url)))
})

describe('getProjectGroupSubtreeIds wasm wrapper — before ready', () => {
  it('degrades to the root alone (under-scope, never over-scope)', () => {
    expect([...preInit]).toEqual(['root'])
  })
})

describe('getProjectGroupSubtreeIds (orca-config wasm)', () => {
  it('collects the root plus all transitive descendants', () => {
    expect([...getProjectGroupSubtreeIds(GROUPS, 'root')].sort()).toEqual([
      'child',
      'grandchild',
      'root'
    ])
  })

  it('a root absent from the group list still yields itself', () => {
    expect([...getProjectGroupSubtreeIds(GROUPS, 'zzz')]).toEqual(['zzz'])
  })
})
