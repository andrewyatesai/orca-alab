/**
 * Regression pins for the #7715 exec-drop kill/timeout semantics (nushell.md
 * Critic note 1, gate w1-1C). Dropping the leading `exec` from
 * wrapRemoteCommandForPosixShell leaves the remote login shell alive as parent
 * of the /bin/sh child on EVERY POSIX remote, so remote kill paths now signal
 * a tree rooted at that login shell. These tests run the REAL production code
 * paths, not models of them:
 *
 * - runRelayGitRemoteCommand abort + timeout (relay remote-git kill path:
 *   detached-group SIGTERM with SIGKILL escalation). The real SSH loop is
 *   unreachable in unit scope, so a PATH-injected `git` transport shim stands
 *   in for the remote hop and runs the REAL wrapped line under the login
 *   shell exactly as sshd would; the production function is what's under test.
 * - AgentExecHandler `agent.execNonInteractive` timeout (relay hook/agent exec
 *   path: SIGKILL on the immediate login-shell child), in the production hook
 *   shape `<shell> -lc <script>` (worktree-remote.ts preCreate hooks).
 *
 * Shell matrix: bash always (missing bash fails loudly — never skips); fish via
 * a deterministic PATH-injected stub that implements fish's exec semantics:
 * interprets the -c payload in-process, forks external commands as children,
 * never exec's the payload, and forwards no signals. Fish PARSING of the
 * wrapped line is covered against the real binary (when installed) in
 * ssh-remote-command-wrapper.integration.test.ts; this file pins the
 * process/signal topology, which must be deterministic on every POSIX host.
 */
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import { runRelayGitRemoteCommand } from '../../relay/relay-git-remote-command'
import { createHandlers, requestContext } from '../../relay/agent-exec-handler-test-harness'
import { wrapRemoteCommandForPosixShell } from './ssh-connection-utils'

// Skip is environment-only: win32 has no POSIX login shells or process groups.
// On POSIX hosts both shells always run — bash real, fish via the stub.
const describePosix: typeof describe.skip | typeof describe =
  process.platform === 'win32'
    ? (((title: string, fn: () => void) =>
        describe.skip(
          `${title} [skipped: win32 host has no POSIX process-group semantics]`,
          fn
        )) as typeof describe.skip)
    : describe

const tempDirs: string[] = []
const lingeringPgids: number[] = []
const lingeringPids: number[] = []

function makeTempDir(): string {
  const dir = mkdtempSync(join(tmpdir(), 'orca-kill-timeout-'))
  tempDirs.push(dir)
  return dir
}

afterEach(() => {
  // Best-effort teardown so a failing assertion cannot leak `sleep`s into CI.
  for (const pgid of lingeringPgids.splice(0)) {
    try {
      process.kill(-pgid, 'SIGKILL')
    } catch {
      /* group already gone */
    }
  }
  for (const pid of lingeringPids.splice(0)) {
    try {
      process.kill(pid, 'SIGKILL')
    } catch {
      /* process already gone */
    }
  }
  for (const dir of tempDirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true })
  }
})

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

function pidAlive(pid: number): boolean {
  try {
    process.kill(pid, 0)
    return true
  } catch {
    return false
  }
}

function writeExecutable(path: string, content: string): void {
  writeFileSync(path, content, { mode: 0o755 })
}

function resolveBash(): string {
  for (const candidate of ['/bin/bash', '/usr/bin/bash', '/usr/local/bin/bash']) {
    if (existsSync(candidate)) {
      return candidate
    }
  }
  // Why: the Critic note requires bash coverage always — a missing bash is a
  // broken environment, never a silent-skip condition.
  throw new Error('bash not found; kill/timeout regression tests require bash (no silent skip)')
}

// Why `exit $?`: without a trailing command, sh would tail-exec the payload's
// final external and the stub would not survive as parent — the exact topology
// difference this suite exists to pin.
const FISH_STUB = `#!/bin/sh
payload=
while [ $# -gt 0 ]; do
  case "$1" in
    -c) payload=$2; shift 2 ;;
    -lc) payload=$2; shift 2 ;;
    -l) shift ;;
    *) printf 'fish-stub: unsupported argument %s\\n' "$1" >&2; exit 64 ;;
  esac
done
if [ -z "$payload" ]; then
  printf 'fish-stub: missing -c payload\\n' >&2
  exit 64
fi
eval "$payload"
exit $?
`

type LoginShellCase = {
  label: string
  /** Bare binary name, resolved through the injected PATH like production hooks. */
  binaryName: string
  /** Absolute shell path; materializes the fish stub into the fixture dir. */
  resolvePath: (fixtureDir: string) => string
}

const SHELL_CASES: LoginShellCase[] = [
  { label: 'bash', binaryName: 'bash', resolvePath: () => resolveBash() },
  {
    label: 'fish (deterministic stub)',
    binaryName: 'fish',
    resolvePath: (fixtureDir) => {
      const stubPath = join(fixtureDir, 'fish')
      writeExecutable(stubPath, FISH_STUB)
      return stubPath
    }
  }
]

type RelayFixture = {
  dir: string
  env: NodeJS.ProcessEnv
  pgidFile: string
  leaderPidFile: string
  doneFile: string
}

function buildRelayFixture(shellCase: LoginShellCase): RelayFixture {
  const dir = makeTempDir()
  const loginShellPath = shellCase.resolvePath(dir)
  const pgidFile = join(dir, 'pgid')
  const leaderPidFile = join(dir, 'leader-pid')
  const doneFile = join(dir, 'done')
  // A long-running remote command; `done` is written only if the kill fails.
  const inner = `ps -o pgid= -p $$ > '${pgidFile}'; sleep 30; echo finished > '${doneFile}'`
  // PATH-injected transport shim: runRelayGitRemoteCommand resolves `git` from
  // options.env PATH. The shim records the detached group leader pid (the pid
  // the production kill path signals as `-pid`), then execs the login shell on
  // the REAL wrapped line — the same argv shape sshd hands a remote login shell.
  writeExecutable(
    join(dir, 'git'),
    '#!/bin/sh\n' +
      'printf %s "$$" > "$ORCA_TEST_LEADER_PID_FILE"\n' +
      'exec "$ORCA_TEST_LOGIN_SHELL" -c "$ORCA_TEST_WRAPPED_LINE"\n'
  )
  return {
    dir,
    env: {
      PATH: `${dir}:/usr/bin:/bin`,
      ORCA_TEST_LOGIN_SHELL: loginShellPath,
      ORCA_TEST_WRAPPED_LINE: wrapRemoteCommandForPosixShell(inner),
      ORCA_TEST_LEADER_PID_FILE: leaderPidFile
    },
    pgidFile,
    leaderPidFile,
    doneFile
  }
}

async function readStartedGroup(
  fixture: RelayFixture
): Promise<{ pgid: number; leaderPid: number }> {
  await waitFor(
    () => existsSync(fixture.pgidFile) && existsSync(fixture.leaderPidFile),
    10_000,
    'wrapped remote command start'
  )
  const pgid = Number(readFileSync(fixture.pgidFile, 'utf8').trim())
  const leaderPid = Number(readFileSync(fixture.leaderPidFile, 'utf8').trim())
  lingeringPgids.push(pgid)
  return { pgid, leaderPid }
}

describePosix(
  'relay-abort path: runRelayGitRemoteCommand tears down the surviving login-shell tree (#7715)',
  () => {
    for (const shellCase of SHELL_CASES) {
      it(`${shellCase.label}: abort group-SIGTERMs login shell, sh child, and command mid-flight`, async () => {
        const fixture = buildRelayFixture(shellCase)
        const controller = new AbortController()
        const pending = runRelayGitRemoteCommand(['fetch', 'origin'], {
          cwd: fixture.dir,
          env: fixture.env,
          maxBuffer: 1024 * 1024,
          signal: controller.signal,
          timeout: 60_000
        })
        // Attach the handler up front so an early rejection is never unhandled.
        const settledError = pending.then(
          () => null,
          (error: Error) => error
        )
        const { pgid, leaderPid } = await readStartedGroup(fixture)
        // Pin: post-exec-drop the decoded command still runs inside the login
        // shell's process group — the group the production abort path signals.
        expect(pgid, `under ${shellCase.label}`).toBe(leaderPid)
        controller.abort()
        const error = await settledError
        expect(error).toBeInstanceOf(Error)
        expect(error?.name).toBe('AbortError')
        expect(error?.message).toBe('The operation was aborted.')
        await waitFor(
          () => !processGroupAlive(pgid),
          5_000,
          `${shellCase.label} group teardown after abort`
        )
        // The command died mid-flight; the login shell forwarded nothing extra.
        expect(existsSync(fixture.doneFile), `under ${shellCase.label}`).toBe(false)
      }, 30_000)

      it(`${shellCase.label}: relay timeout rejects 'git timed out.' and the wrapped tree dies`, async () => {
        const fixture = buildRelayFixture(shellCase)
        const pending = runRelayGitRemoteCommand(['push', 'origin', 'HEAD'], {
          cwd: fixture.dir,
          env: fixture.env,
          maxBuffer: 1024 * 1024,
          timeout: 1_500
        })
        const settledError = pending.then(
          () => null,
          (error: Error) => error
        )
        const { pgid, leaderPid } = await readStartedGroup(fixture)
        expect(pgid, `under ${shellCase.label}`).toBe(leaderPid)
        const error = await settledError
        expect(error?.message).toBe('git timed out.')
        await waitFor(
          () => !processGroupAlive(pgid),
          5_000,
          `${shellCase.label} group teardown after timeout`
        )
        expect(existsSync(fixture.doneFile), `under ${shellCase.label}`).toBe(false)
      }, 30_000)
    }
  }
)

describePosix(
  'execNonInteractive timeout path: AgentExecHandler kills the login-shell hook child',
  () => {
    for (const shellCase of SHELL_CASES) {
      it(`${shellCase.label}: timeout resolves timedOut:true and the interpreting shell dies, so trailing hook commands never run`, async () => {
        const dir = makeTempDir()
        shellCase.resolvePath(dir) // materialize the fish stub onto the injected PATH
        const shellPidFile = join(dir, 'shell-pid')
        const doneFile = join(dir, 'done')
        // Production hook shape (worktree-remote preCreate): `<shell> -lc <script>`.
        // $$ is the pid of the shell interpreting the hook == the spawned child.
        const script = `printf %s "$$" > '${shellPidFile}'; sleep 15; echo finished > '${doneFile}'`
        const handlers = createHandlers()
        const pending = handlers.get('agent.execNonInteractive')!(
          {
            binary: shellCase.binaryName,
            args: ['-lc', script],
            cwd: dir,
            stdin: null,
            timeoutMs: 1_000,
            env: { PATH: `${dir}:/usr/bin:/bin`, HOME: dir }
          },
          requestContext()
        )
        await waitFor(() => existsSync(shellPidFile), 10_000, 'hook script start')
        const shellPid = Number(readFileSync(shellPidFile, 'utf8').trim())
        lingeringPids.push(shellPid)
        const result = (await pending) as {
          timedOut: boolean
          exitCode: number | null
          canceled?: boolean
        }
        expect(result, `under ${shellCase.label}`).toMatchObject({ timedOut: true, exitCode: null })
        expect(result.canceled, `under ${shellCase.label}`).toBeFalsy()
        // The SIGKILL must land on the shell actually interpreting the hook.
        await waitFor(
          () => !pidAlive(shellPid),
          5_000,
          `${shellCase.label} hook interpreter death`
        )
        // Only the (now dead) interpreter writes `done`, so the sequence can
        // never resume after the timeout. (The forked `sleep` may linger
        // briefly as an orphan; it writes nothing and exits on its own.)
        expect(existsSync(doneFile), `under ${shellCase.label}`).toBe(false)
      }, 30_000)
    }
  }
)
