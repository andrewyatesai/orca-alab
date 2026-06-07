// TS dispatch for the github-pr-merge-methods parity module: maps the shared
// vector function names to the real `src/shared/github-pr-merge-methods.ts`
// exports so the harness compares the live TS reference against the Rust port.

import {
  mapGitHubDefaultMergeMethod,
  normalizeGitHubPRMergeMethodSettings,
  resolveGitHubPRMergeMethods
} from '../../../src/shared/github-pr-merge-methods'
import type { GitHubPRMergeMethodSettings } from '../../../src/shared/types'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'mapGitHubDefaultMergeMethod':
      return mapGitHubDefaultMergeMethod(input)
    case 'normalizeGitHubPRMergeMethodSettings': {
      const { defaultMethod, mergeCommitAllowed, rebaseMergeAllowed, squashMergeAllowed } =
        input as {
          defaultMethod: unknown
          mergeCommitAllowed: unknown
          rebaseMergeAllowed: unknown
          squashMergeAllowed: unknown
        }
      return normalizeGitHubPRMergeMethodSettings({
        defaultMethod,
        mergeCommitAllowed,
        rebaseMergeAllowed,
        squashMergeAllowed
      })
    }
    case 'resolveGitHubPRMergeMethods':
      return resolveGitHubPRMergeMethods(input as GitHubPRMergeMethodSettings | null | undefined)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
