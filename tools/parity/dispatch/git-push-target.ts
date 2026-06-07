// TS dispatch for the git-push-target parity module: maps the shared vector
// function names to the real `src/shared/git-push-target-validation.ts` export.
// `assertGitPushTargetShape` throws on invalid input and returns void on success,
// so we wrap it into a `{ ok, error? }` value to match the Rust `Result` image.

import { assertGitPushTargetShape } from '../../../src/shared/git-push-target-validation'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'assertGitPushTargetShape': {
      const { remoteName, branchName, remoteUrl } = input as {
        remoteName: string
        branchName: string
        remoteUrl?: string
      }
      // Omit remoteUrl when absent so the optional-URL branch is skipped,
      // matching the Rust `None` arm (an explicit `undefined` key would too).
      const target: Record<string, unknown> = { remoteName, branchName }
      if (remoteUrl !== undefined) {
        target.remoteUrl = remoteUrl
      }
      try {
        assertGitPushTargetShape(target)
        return { ok: true }
      } catch (error) {
        return { ok: false, error: (error as Error).message }
      }
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
