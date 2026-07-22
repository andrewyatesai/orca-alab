/**
 * Real-shell contract for wrapRemoteCommandForPosixShell after the exec-drop
 * (#7715 / nushell PR2). sshd runs the wrapped line as `$SHELL -c '<line>'`;
 * these tests replay that with every login shell present on the host.
 *
 * Critic note 1 (nushell.md): dropping `exec` changes process topology for
 * EVERY POSIX remote — the login shell survives as parent of the sh child.
 * The signal/exit suites pin the semantics remote kill/timeout paths rely on:
 * group signals still reach the command, and the command's exit code still
 * propagates to the exec channel.
 */
import { spawn, spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import { wrapRemoteCommandForPosixShell } from './ssh-connection-utils'

const describePosix = process.platform === 'win32' ? describe.skip : describe

// Every shell class sshd hands the exec line to: POSIX, csh-family, fish, nu.
const CANDIDATE_LOGIN_SHELLS = ['sh', 'bash', 'dash', 'zsh', 'fish', 'csh', 'tcsh', 'nu']

function resolveShell(name: string): string | null {
  if (process.platform === 'win32') {
    return null
  }
  const result = spawnSync('which', [name], { encoding: 'utf8' })
  return result.status === 0 ? result.stdout.trim().split('\n')[0] : null
}

const AVAILABLE_LOGIN_SHELLS = CANDIDATE_LOGIN_SHELLS.map(resolveShell).filter(
  (shell): shell is string => shell !== null
)

const tempDirs: string[] = []

function makeTempDir(): string {
  const dir = mkdtempSync(join(tmpdir(), 'orca-ssh-wrapper-'))
  tempDirs.push(dir)
  return dir
}

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true })
  }
})

function runWrappedUnder(loginShell: string, command: string): ReturnType<typeof spawnSync> {
  // sshd shape: the remote login shell parses the single wrapped line.
  return spawnSync(loginShell, ['-c', wrapRemoteCommandForPosixShell(command)], {
    encoding: 'utf8',
    timeout: 15_000
  })
}

async function waitFor(predicate: () => boolean, timeoutMs: number, what: string): Promise<void> {
  const deadline = Date.now() + timeoutMs
  while (!predicate()) {
    if (Date.now() > deadline) {
      throw new Error(`timed out waiting for ${what}`)
    }
    await new Promise((resolve) => setTimeout(resolve, 25))
  }
}

function processGroupAlive(pgid: number): boolean {
  try {
    process.kill(-pgid, 0)
    return true
  } catch {
    return false
  }
}

describePosix('wrapped remote command parse matrix (real login shells)', () => {
  it('runs under every available login shell dialect', () => {
    expect(AVAILABLE_LOGIN_SHELLS.length).toBeGreaterThan(0)
    for (const shell of AVAILABLE_LOGIN_SHELLS) {
      const result = runWrappedUnder(shell, 'echo wrapper-matrix-ok')
      expect(result.status, `${shell} stderr: ${result.stderr}`).toBe(0)
      expect(result.stdout, `under ${shell}`).toContain('wrapper-matrix-ok')
    }
  })

  it('carries quote-bearing, bang, and backslash payloads intact (adversarial)', () => {
    const payload = `it's a "test" with !bang and back\\slash`
    for (const shell of AVAILABLE_LOGIN_SHELLS) {
      const result = runWrappedUnder(shell, `printf '%s\\n' ${JSON.stringify(payload)}`)
      expect(result.status, `${shell} stderr: ${result.stderr}`).toBe(0)
      expect(result.stdout, `under ${shell}`).toContain(payload)
    }
  })

  it('reassembles >1 KiB commands split across printf chunks', () => {
    const longValue = 'y'.repeat(2500)
    for (const shell of AVAILABLE_LOGIN_SHELLS) {
      const result = runWrappedUnder(shell, `echo begin-${longValue}-end`)
      expect(result.status, `${shell} stderr: ${result.stderr}`).toBe(0)
      expect(result.stdout, `under ${shell}`).toContain(`begin-${longValue}-end`)
    }
  })
})

describePosix('wrapped remote command kill/timeout semantics (Critic note 1)', () => {
  // Why: execCommand rejects on `code !== 0` — the login-shell parent must keep forwarding the command's exit code.
  it('propagates the inner exit code through the surviving login-shell parent', () => {
    for (const shell of AVAILABLE_LOGIN_SHELLS) {
      const result = runWrappedUnder(shell, 'exit 42')
      expect(result.status, `under ${shell}`).toBe(42)
    }
  })

  // Why: sshd tears sessions down with process-group signals; the sh child must stay in the
  // login shell's group now that exec no longer replaces the login shell.
  it('keeps the sh child in the login shell process group and dies on a group signal', async () => {
    for (const shell of AVAILABLE_LOGIN_SHELLS) {
      const dir = makeTempDir()
      const pgidFile = join(dir, 'pgid')
      const doneFile = join(dir, 'done')
      const inner = `ps -o pgid= -p $$ > ${pgidFile}; sleep 30; echo finished > ${doneFile}`

      const child = spawn(shell, ['-c', wrapRemoteCommandForPosixShell(inner)], {
        stdio: 'ignore',
        // Own group, standing in for sshd's per-session process group.
        detached: true
      })
      expect(typeof child.pid).toBe('number')
      const sessionPgid = child.pid!

      await waitFor(() => existsSync(pgidFile), 10_000, `${shell} wrapped command start`)
      const innerPgid = Number(readFileSync(pgidFile, 'utf8').trim())
      // Pin: the decoded command runs inside the SAME process group as the login shell.
      expect(innerPgid, `under ${shell}`).toBe(sessionPgid)

      process.kill(-sessionPgid, 'SIGTERM')
      await waitFor(() => !processGroupAlive(sessionPgid), 5_000, `${shell} process group teardown`)
      // The long-running command was killed mid-flight — it never completed.
      expect(existsSync(doneFile), `under ${shell}`).toBe(false)
    }
  }, 60_000)
})
