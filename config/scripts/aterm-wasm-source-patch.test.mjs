import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'
import {
  ATERM_WASM_SOURCE_PATCH_PATH,
  assertAtermWasmSourcePatchApplies,
  expectedAtermWasmSourcePatch,
  validateAtermWasmSourcePatch,
  withPatchedAtermWorktree
} from './aterm-wasm-source-patch.mjs'

const ROOT = resolve(import.meta.dirname, '../..')
const ATERM_SOURCE = resolve(ROOT, 'rust/aterm')

function git(args) {
  return execFileSync('git', ['-C', ATERM_SOURCE, ...args], { encoding: 'utf8' }).trim()
}

describe('aterm wasm source patch', () => {
  it('pins the checked-in patch by path and SHA-256', () => {
    const expected = expectedAtermWasmSourcePatch(ROOT)

    expect(expected.path).toBe(ATERM_WASM_SOURCE_PATCH_PATH)
    expect(expected.sha256).toMatch(/^[0-9a-f]{64}$/)
    expect(validateAtermWasmSourcePatch(expected, ROOT)).toEqual([])
    expect(validateAtermWasmSourcePatch({ ...expected, sha256: '0'.repeat(64) }, ROOT)).toEqual([
      'aterm wasm source patch does not match its exact SHA-256 pin'
    ])
  })

  it('applies only in a detached temporary worktree and cleans it after failure', async () => {
    const sourceCommit = git(['rev-parse', 'HEAD'])
    const mainStatusBefore = git(['status', '--porcelain'])
    let temporaryWorktree = ''

    assertAtermWasmSourcePatchApplies(ROOT, ATERM_SOURCE)
    await expect(
      withPatchedAtermWorktree(
        { root: ROOT, atermSource: ATERM_SOURCE, sourceCommit },
        async (worktree) => {
          temporaryWorktree = worktree
          const source = readFileSync(resolve(worktree, 'crates/aterm-gpu/src/renderer.rs'), 'utf8')
          expect(source.match(/let work_started = web_time::Instant::now\(\);/g)).toHaveLength(2)
          expect(source.match(/let work_started = std::time::Instant::now\(\);/g)).toBeNull()
          expect(
            execFileSync('git', ['-C', worktree, 'status', '--porcelain'], {
              encoding: 'utf8'
            }).trim()
          ).toBe('M crates/aterm-gpu/src/renderer.rs')
          throw new Error('intentional cleanup probe')
        }
      )
    ).rejects.toThrow('intentional cleanup probe')

    expect(temporaryWorktree).not.toBe('')
    expect(existsSync(temporaryWorktree)).toBe(false)
    expect(git(['status', '--porcelain'])).toBe(mainStatusBefore)
    expect(git(['worktree', 'list', '--porcelain'])).not.toContain(temporaryWorktree)
  })
})
