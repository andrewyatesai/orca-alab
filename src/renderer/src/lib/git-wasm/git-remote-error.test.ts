import { readFileSync } from 'node:fs'
import { beforeAll, describe, expect, it } from 'vitest'
import { formatSubmodulePushFailureDetail, stripCredentialsFromMessage } from './git-remote-error'
import { initGitWasmForTestFromBytes, isGitWasmReady } from './git-line-stats'

// Captured at import time, BEFORE init: pins the null-until-ready contract —
// callers must SKIP details rather than show an unscrubbed message.
const preInitReady = isGitWasmReady()
const preInitStripped = stripCredentialsFromMessage('https://user:secret@host/repo fatal: nope')
const preInitDetail = formatSubmodulePushFailureDetail(
  'fatal: failed to push all needed submodules'
)

beforeAll(() => {
  initGitWasmForTestFromBytes(readFileSync(new URL('./orca_git_wasm_bg.wasm', import.meta.url)))
})

describe('renderer git remote-error helpers (orca-git wasm)', () => {
  it('returns null for both helpers before the wasm is initialised', () => {
    expect(preInitReady).toBe(false)
    expect(preInitStripped).toBeNull()
    expect(preInitDetail).toBeNull()
  })

  it('scrubs credentials embedded in git URLs', () => {
    expect(stripCredentialsFromMessage('fetch https://user:secret@host/repo failed')).toBe(
      'fetch https://host/repo failed'
    )
  })

  // Ported from the deleted src/shared/git-remote-error.test.ts (the spy-based
  // TS-internal assertions were dropped with the TS implementation).
  it('keeps normalized guidance when transport layers prefix the error', () => {
    expect(
      formatSubmodulePushFailureDetail(
        "Error invoking remote method 'git:push': Error: Submodule 'vendor/tools' has remote changes. Pull inside the submodule, then try again."
      )
    ).toBe(
      "Submodule 'vendor/tools' has remote changes. Pull inside the submodule, then try again."
    )
  })

  it('falls back to submodule-specific guidance when git omits the nested reason', () => {
    expect(
      formatSubmodulePushFailureDetail(
        "Unable to push submodule 'vendor/tools'\nfatal: failed to push all needed submodules"
      )
    ).toBe(
      "Submodule 'vendor/tools' could not be pushed. Resolve the submodule push error, then try again."
    )
  })

  it('handles newline-heavy CRLF output', () => {
    const message = `${'remote: progress\r\n'.repeat(10_000)}Unable to push submodule 'vendor/tools'\r\nfatal: failed to push all needed submodules\r\n`

    expect(formatSubmodulePushFailureDetail(message)).toBe(
      "Submodule 'vendor/tools' could not be pushed. Resolve the submodule push error, then try again."
    )
  })

  it('returns null when there is no submodule failure', () => {
    expect(formatSubmodulePushFailureDetail('fatal: unrelated failure')).toBeNull()
  })
})
