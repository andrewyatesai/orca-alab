import { describe, expect, it } from 'vitest'
import { assertGitPushTargetShape } from '../../shared/git-push-target-validation'
import { assertGitPushTargetShapeNative } from './rust-push-target-validation'

// The Rust-preferring wrapper must be a byte-for-byte behavioural drop-in for the
// canonical pure-TS validator — same accept/reject AND same error message. This
// covers the value rules, the unknown→typed guards, and the ordering subtlety the
// golden vectors don't reach: remoteUrl's type guard is deferred until after the
// name/branch value rules. When the addon is present this exercises the Rust path;
// when absent the wrapper falls through to the canonical validator.

function outcome(
  validate: (target: unknown) => void,
  input: unknown
): { ok: boolean; error?: string } {
  try {
    validate(input)
    return { ok: true }
  } catch (error) {
    return { ok: false, error: error instanceof Error ? error.message : String(error) }
  }
}

const cases: unknown[] = [
  // Valid
  { remoteName: 'origin', branchName: 'main' },
  { remoteName: 'foo/bar', branchName: 'feature/fix' },
  { remoteName: 'origin', branchName: 'main', remoteUrl: 'https://github.com/owner/repo.git' },
  { remoteName: 'origin', branchName: 'main', remoteUrl: 'git@github.com:owner/repo.git' },
  { remoteName: 'origin', branchName: 'main', remoteUrl: undefined }, // absent URL
  // Invalid remote name (value rules)
  { remoteName: 'foo//bar', branchName: 'x' },
  { remoteName: 'foo/../bar', branchName: 'x' },
  { remoteName: '', branchName: 'x' },
  { remoteName: '.', branchName: 'x' },
  { remoteName: `${'a'.repeat(101)}`, branchName: 'x' },
  { remoteName: 'bad name', branchName: 'x' },
  // Invalid branch name (value rules)
  { remoteName: 'origin', branchName: '-rf' },
  { remoteName: 'origin', branchName: '' },
  // Invalid URL (value rules)
  { remoteName: 'origin', branchName: 'main', remoteUrl: 'https://gitlab.com/o/r.git' },
  { remoteName: 'origin', branchName: 'main', remoteUrl: 'http://github.com/o/r.git' },
  { remoteName: 'origin', branchName: 'main', remoteUrl: 'not a url' },
  // Type guards (unknown→typed)
  null,
  undefined,
  42,
  'origin',
  {},
  { branchName: 'main' },
  { remoteName: 'origin' },
  { remoteName: 123, branchName: 'main' },
  { remoteName: 'origin', branchName: 123 },
  { remoteName: 'origin', branchName: 'main', remoteUrl: 123 },
  // Ordering: a name/branch value error must win over the deferred URL type guard
  { remoteName: 'bad name', branchName: 'main', remoteUrl: 123 },
  { remoteName: 'origin', branchName: '-x', remoteUrl: 123 },
  // Ordering: valid name/branch + non-string URL → the deferred URL type guard
  { remoteName: 'origin', branchName: 'main', remoteUrl: {} }
]

describe('assertGitPushTargetShapeNative', () => {
  for (const [i, input] of cases.entries()) {
    it(`${i}: ${JSON.stringify(input) ?? String(input)}`, () => {
      expect(outcome(assertGitPushTargetShapeNative, input)).toEqual(
        outcome(assertGitPushTargetShape, input)
      )
    })
  }
})
