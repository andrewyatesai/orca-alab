import { describe, expect, it } from 'vitest'
import { isNoUpstreamError, normalizeGitErrorMessage } from './rust-git-remote-error'

// Ported from the deleted src/shared/git-remote-error.test.ts: the same
// behavioural expectations now run THROUGH the Rust orca-text core via napi
// (the relay runs the identical functions via wasm). The TS-internal
// split/replace spy assertions were dropped with the TS implementation.

describe('normalizeGitErrorMessage (Rust napi)', () => {
  it('keeps the submodule name when a recursive push is rejected', () => {
    const error = new Error(
      "Command failed: git push\nPushing submodule 'find-cmux-followers'\n" +
        'To https://github.com/stablyai/orca-internal\n' +
        ' ! [rejected]        master -> master (fetch first)\n' +
        "Unable to push submodule 'find-cmux-followers'\n" +
        'fatal: failed to push all needed submodules'
    )

    expect(normalizeGitErrorMessage(error, 'push')).toBe(
      "Submodule 'find-cmux-followers' has remote changes. Pull inside the submodule, then try again."
    )
  })

  it('explains how to configure a pull policy for divergent branches', () => {
    const error = new Error(
      'Command failed: git pull\n' +
        'hint: You have divergent branches and need to specify how to reconcile them.\n' +
        'fatal: Need to specify how to reconcile divergent branches.'
    )

    expect(normalizeGitErrorMessage(error, 'pull')).toBe(
      'Pull needs a Git pull policy for divergent branches. Configure one for this repository ' +
        'or host, then try again: git config pull.rebase false (merge), ' +
        'git config pull.rebase true (rebase), or git config pull.ff only (fast-forward only).'
    )
  })

  it('uses the tail diagnostic from newline-heavy failures', () => {
    const error = new Error(
      `Command failed: git fetch\r\n${'remote: progress update\r\n'.repeat(10_000)}remote side closed connection\r\n`
    )

    expect(normalizeGitErrorMessage(error, 'fetch')).toBe('remote side closed connection')
  })

  it('returns the fixed fallback for a non-Error throw', () => {
    expect(normalizeGitErrorMessage('string throw')).toBe('Git remote operation failed.')
  })
})

describe('isNoUpstreamError (Rust napi)', () => {
  it('treats a missing HEAD@{u} tracking ref as no upstream', () => {
    const error = new Error(
      "fatal: ambiguous argument 'HEAD@{u}': unknown revision or path not in the working tree.\n" +
        "Use '--' to separate paths from revisions, like this:\n" +
        "'git <command> [<revision>...] -- [<file>...]'"
    )

    expect(isNoUpstreamError(error)).toBe(true)
  })

  it('does not treat unrelated ambiguous refs as no upstream', () => {
    const error = new Error(
      "fatal: ambiguous argument 'feature': unknown revision or path not in the working tree."
    )

    expect(isNoUpstreamError(error)).toBe(false)
  })
})
