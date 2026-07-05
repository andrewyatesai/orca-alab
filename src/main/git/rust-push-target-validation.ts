import type { GitPushTarget } from '../../shared/types'
import { assertGitPushTargetShape } from '../../shared/git-push-target-validation'
import { loadRustGitBinding } from '../daemon/rust-git-addon'

// Main-only Rust-preferring wrapper for push-target validation. The substantive
// path-traversal-safety rules (remote name / branch name / GitHub URL) run in the
// verified `orca_core` validator behind the napi boundary; the pure TS
// `assertGitPushTargetShape` is the proven-identical fallback (shared with the
// relay + parity harness, which must NOT depend on the native addon). Only the
// `unknown`→typed guards live here in JS — they own the JS-boundary "Invalid PR
// push target …" messages the Rust value-rule validator never produces.

function assertString(value: unknown, name: string): asserts value is string {
  if (typeof value !== 'string') {
    throw new Error(`Invalid PR push target ${name}.`)
  }
}

/**
 * Prefer the Rust value-rule validator, falling back to the pure TS validator
 * when the addon is unavailable. Behaviour — including exact error message and
 * ordering — is identical to {@link assertGitPushTargetShape}.
 */
export function assertGitPushTargetShapePreferRust(
  target: unknown
): asserts target is GitPushTarget {
  const binding = loadRustGitBinding()
  if (!binding?.validateGitPushTargetRules) {
    assertGitPushTargetShape(target)
    return
  }

  // Type-coercion guards, in the order assertGitPushTargetShape applies them:
  // object, then remoteName/branchName strings. remoteUrl's type guard is
  // deliberately deferred below — the canonical validator runs it AFTER the
  // name/branch value rules, so preserving that ordering keeps error parity when
  // a target is both name-invalid and carries a non-string URL.
  if (typeof target !== 'object' || target === null) {
    throw new Error('Invalid PR push target.')
  }
  const candidate = target as Record<string, unknown>
  assertString(candidate.remoteName, 'remote name')
  assertString(candidate.branchName, 'branch name')

  const urlPresent = candidate.remoteUrl !== undefined
  const urlIsString = typeof candidate.remoteUrl === 'string'
  // Pass the URL to Rust only when it is a present string; a present-but-non-string
  // URL is left for the deferred type guard so its message fires in the right order.
  const error = binding.validateGitPushTargetRules(
    candidate.remoteName,
    candidate.branchName,
    urlPresent && urlIsString ? (candidate.remoteUrl as string) : null
  )
  if (error) {
    throw new Error(error)
  }
  // Name + branch (and a string URL) passed. The only remaining case is a
  // present non-string URL — the deferred guard, matching assertString's message.
  if (urlPresent && !urlIsString) {
    throw new Error('Invalid PR push target remote URL.')
  }
}
