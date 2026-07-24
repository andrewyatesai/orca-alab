/**
 * Regression: gitExecFileAsync must forward `stdin` even on the killProcessTree
 * path. On non-Windows that path routes through spawnCommandCapture, which used
 * `stdio: ['ignore', ...]` and dropped stdin — git then read EOF and could exit 0
 * as a no-op (a real failure misread as success). Drives the real runner + git.
 */
import { describe, expect, it } from 'vitest'
import { gitExecFileAsync } from './runner'

describe('gitExecFileAsync forwards stdin on the killProcessTree path', () => {
  it('feeds stdin to git (stripspace echoes it) instead of dropping it to EOF', async () => {
    // `git stripspace` echoes cleaned stdin to stdout; empty input yields empty output.
    const { stdout } = await gitExecFileAsync(['stripspace'], {
      cwd: process.cwd(),
      stdin: 'payload-line\n',
      killProcessTree: true
    })
    expect(stdout).toContain('payload-line')
  })

  it('still forwards stdin without killProcessTree (execFileCapture path)', async () => {
    const { stdout } = await gitExecFileAsync(['stripspace'], {
      cwd: process.cwd(),
      stdin: 'other-payload\n'
    })
    expect(stdout).toContain('other-payload')
  })
})
