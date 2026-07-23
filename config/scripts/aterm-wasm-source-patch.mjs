import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'

export const ATERM_WASM_SOURCE_PATCH_PATH = 'config/patches/aterm-wasm-source-fixes.patch'

export function sha256File(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex')
}

export function expectedAtermWasmSourcePatch(root) {
  const absolutePath = resolve(root, ATERM_WASM_SOURCE_PATCH_PATH)
  if (!existsSync(absolutePath)) {
    throw new Error(`required aterm wasm source patch is missing: ${ATERM_WASM_SOURCE_PATCH_PATH}`)
  }
  return {
    path: ATERM_WASM_SOURCE_PATCH_PATH,
    sha256: sha256File(absolutePath)
  }
}

export function validateAtermWasmSourcePatch(actual, root) {
  const expected = expectedAtermWasmSourcePatch(root)
  const mismatches = []
  if (!actual || typeof actual !== 'object') {
    return ['artifact manifest does not identify its aterm wasm source patch']
  }
  if (actual.path !== expected.path) {
    mismatches.push(
      `artifact manifest names source patch ${String(actual.path)} but the build uses ${expected.path}`
    )
  }
  if (actual.sha256 !== expected.sha256) {
    mismatches.push('aterm wasm source patch does not match its exact SHA-256 pin')
  }
  return mismatches
}

function git(cwd, args, options = {}) {
  return execFileSync('git', ['-C', cwd, ...args], options)
}

export function assertAtermWasmSourcePatchApplies(root, atermSource) {
  const patch = resolve(root, ATERM_WASM_SOURCE_PATCH_PATH)
  try {
    git(atermSource, ['apply', '--check', patch], { stdio: 'pipe' })
  } catch (error) {
    const detail = error?.stderr?.toString().trim()
    throw new Error(
      `aterm wasm source patch does not apply to ${atermSource}${detail ? `: ${detail}` : ''}`
    )
  }
}

/**
 * Materialize the pinned aterm commit in a detached throwaway worktree, apply
 * the downstream wasm-only fix there, and always remove it after `callback`.
 * The checked-out rust/aterm submodule is never modified, including when Cargo
 * fails or is interrupted.
 */
export async function withPatchedAtermWorktree({ root, atermSource, sourceCommit }, callback) {
  const temporaryRoot = mkdtempSync(join(tmpdir(), 'orca-aterm-wasm-'))
  const worktree = join(temporaryRoot, 'aterm')
  const patch = resolve(root, ATERM_WASM_SOURCE_PATCH_PATH)
  let worktreeRegistered = false
  try {
    git(atermSource, ['worktree', 'add', '--quiet', '--detach', worktree, sourceCommit], {
      stdio: 'pipe'
    })
    worktreeRegistered = true
    git(worktree, ['apply', '--check', patch], { stdio: 'pipe' })
    git(worktree, ['apply', patch], { stdio: 'pipe' })
    return await callback(worktree)
  } finally {
    if (worktreeRegistered) {
      try {
        git(atermSource, ['worktree', 'remove', '--force', worktree], { stdio: 'pipe' })
      } catch {
        // Remove the path first, then prune any registration left by an
        // interrupted/failed `worktree remove` so later builds stay clean.
        rmSync(worktree, { recursive: true, force: true })
        try {
          git(atermSource, ['worktree', 'prune'], { stdio: 'pipe' })
        } catch {
          // Preserve the original build error. A later Git command can prune it.
        }
      }
    }
    rmSync(temporaryRoot, { recursive: true, force: true })
  }
}
