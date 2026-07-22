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
import { SshConnection } from './ssh-connection'
import { execCommand, isUnconfirmedSshCommandTermination } from './ssh-relay-exec-command'
import type { SshTarget } from '../../shared/ssh-types'

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
          timeout: 5_000
        })
        const settledError = pending.then(
          () => null,
          (error: Error) => error
        )
        // Why: the timeout timer runs from spawn and can kill the tree before the
        // deep chain writes the pgid file under sweep load (gate flake); gate on the
        // shim's own pid write. The abort test pins pgid===leaderPid race-free.
        await waitFor(() => existsSync(fixture.leaderPidFile), 10_000, 'relay transport shim start')
        const leaderPid = Number(readFileSync(fixture.leaderPidFile, 'utf8').trim())
        lingeringPgids.push(leaderPid)
        const error = await settledError
        expect(error?.message).toBe('git timed out.')
        // Why: detached spawn makes the shim the group leader, so -leaderPid is the
        // group the production timeout path signals.
        await waitFor(
          () => !processGroupAlive(leaderPid),
          5_000,
          `${shellCase.label} group teardown after timeout`
        )
        expect(existsSync(fixture.doneFile), `under ${shellCase.label}`).toBe(false)
      }, 30_000)
    }
  }
)

type SystemSshExecFixture = {
  dir: string
  sshShimPath: string
  inner: string
  sshPidFile: string
  shPidFile: string
  wrappedLineFile: string
  doneFile: string
}

// Why: the ssh2 loop needs a live sshd; ORCA_SYSTEM_SSH_PATH is the production
// seam that lets the REAL execCommand → SshConnection.exec →
// spawnTrackedSystemSshCommand → spawnSystemSshCommand chain run against a
// local stand-in. The shim receives the argv OpenSSH would get — the wrapped
// line is its final argument, exactly what sshd hands the remote login shell —
// and execs the login shell on it with stdio detached like a remote hop.
function buildSystemSshExecFixture(shellCase: LoginShellCase): SystemSshExecFixture {
  const dir = makeTempDir()
  const loginShellPath = shellCase.resolvePath(dir)
  const sshPidFile = join(dir, 'ssh-pid')
  const shPidFile = join(dir, 'sh-pid')
  const wrappedLineFile = join(dir, 'wrapped-line')
  const doneFile = join(dir, 'done')
  const inner = `printf %s "$$" > '${shPidFile}'; sleep 30; echo finished > '${doneFile}'`
  const sshShimPath = join(dir, 'orca-test-ssh')
  writeExecutable(
    sshShimPath,
    '#!/bin/sh\n' +
      'for arg in "$@"; do last=$arg; done\n' +
      `printf %s "$last" > '${wrappedLineFile}'\n` +
      `printf %s "$$" > '${sshPidFile}'\n` +
      // Detach stdio before exec so descendants hold no local pipes — the
      // channel close then reflects only the local ssh process's death, the
      // exact unconfirmed-teardown semantics execCommand must preserve.
      `exec '${loginShellPath}' -c "$last" < /dev/null > /dev/null 2>&1\n`
  )
  return { dir, sshShimPath, inner, sshPidFile, shPidFile, wrappedLineFile, doneFile }
}

// Why: no live connection exists in unit scope; force the post-connect
// system-ssh transport state so exec() takes its production channel path.
function createConnectedSystemSshConnection(): SshConnection {
  const target: SshTarget = {
    id: 'kill-timeout-target',
    label: 'kill-timeout-target',
    host: '127.0.0.1',
    port: 22,
    username: 'orca-test',
    // Why: no ControlMaster socket — each exec must spawn the shim directly.
    systemSshConnectionReuse: false
  }
  const conn = new SshConnection(target, { onStateChange: () => undefined })
  ;(conn as unknown as { useSystemSshTransport: boolean }).useSystemSshTransport = true
  ;(conn as unknown as { state: { status: string } }).state.status = 'connected'
  return conn
}

async function readStartedSystemSshTree(
  fixture: SystemSshExecFixture
): Promise<{ sshPid: number; shPid: number }> {
  await waitFor(
    () => existsSync(fixture.sshPidFile) && existsSync(fixture.shPidFile),
    10_000,
    'system-ssh wrapped command start'
  )
  const sshPid = Number(readFileSync(fixture.sshPidFile, 'utf8').trim())
  const shPid = Number(readFileSync(fixture.shPidFile, 'utf8').trim())
  lingeringPids.push(sshPid, shPid)
  return { sshPid, shPid }
}

describePosix(
  'relay exec path: execCommand → SshConnection.exec channel-close abort/timeout semantics (#7715)',
  () => {
    afterEach(() => {
      delete process.env.ORCA_SYSTEM_SSH_PATH
    })

    for (const shellCase of SHELL_CASES) {
      it(`${shellCase.label}: abort closes the exec channel, SIGTERMs the login shell, and rejects unconfirmed`, async () => {
        const fixture = buildSystemSshExecFixture(shellCase)
        process.env.ORCA_SYSTEM_SSH_PATH = fixture.sshShimPath
        const conn = createConnectedSystemSshConnection()
        const controller = new AbortController()
        const pending = execCommand(conn, fixture.inner, {
          signal: controller.signal,
          timeoutMs: 60_000
        })
        const settledError = pending.then(
          () => null,
          (error: Error) => error
        )
        const { sshPid } = await readStartedSystemSshTree(fixture)
        // Pin: the exec path applied the production wrapper — the shim's final
        // argument is byte-identical to the line sshd would hand a login shell.
        expect(readFileSync(fixture.wrappedLineFile, 'utf8'), `under ${shellCase.label}`).toBe(
          wrapRemoteCommandForPosixShell(fixture.inner)
        )
        controller.abort()
        const error = await settledError
        expect(error).toBeInstanceOf(Error)
        expect(error?.name, `under ${shellCase.label}`).toBe('AbortError')
        expect(error?.message).toBe('SSH operation was cancelled')
        // Pin: a system-ssh channel close proves only the LOCAL process died;
        // callers gating cleanup (install locks) must see it as unconfirmed.
        expect(isUnconfirmedSshCommandTermination(error), `under ${shellCase.label}`).toBe(true)
        // The channel close delivered SIGTERM to the exec'd login shell.
        await waitFor(
          () => !pidAlive(sshPid),
          5_000,
          `${shellCase.label} login-shell death after exec-channel abort`
        )
        expect(existsSync(fixture.doneFile), `under ${shellCase.label}`).toBe(false)
      }, 30_000)

      it(`${shellCase.label}: execCommand timeout closes the channel and rejects with the timeout message`, async () => {
        const fixture = buildSystemSshExecFixture(shellCase)
        process.env.ORCA_SYSTEM_SSH_PATH = fixture.sshShimPath
        const conn = createConnectedSystemSshConnection()
        const pending = execCommand(conn, fixture.inner, { timeoutMs: 5_000 })
        const settledError = pending.then(
          () => null,
          (error: Error) => error
        )
        // Why: the timeout races the shim→login-shell→sh chain under sweep load;
        // gate startup on the shim's own pid write only (1 spawn deep).
        await waitFor(() => existsSync(fixture.sshPidFile), 10_000, 'system-ssh shim start')
        const sshPid = Number(readFileSync(fixture.sshPidFile, 'utf8').trim())
        lingeringPids.push(sshPid)
        const error = await settledError
        // Best-effort orphan cleanup if the inner sh got far enough to record itself.
        if (existsSync(fixture.shPidFile)) {
          lingeringPids.push(Number(readFileSync(fixture.shPidFile, 'utf8').trim()))
        }
        expect(error?.message, `under ${shellCase.label}`).toBe(
          `Command "${fixture.inner}" timed out after 5s`
        )
        expect(isUnconfirmedSshCommandTermination(error), `under ${shellCase.label}`).toBe(true)
        await waitFor(
          () => !pidAlive(sshPid),
          5_000,
          `${shellCase.label} login-shell death after exec-channel timeout`
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
            // Why: the timeout runs from spawn; keep it wide enough that the hook
            // shell's pid write (1 spawn deep) cannot lose the race under load.
            timeoutMs: 5_000,
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
        await waitFor(() => !pidAlive(shellPid), 5_000, `${shellCase.label} hook interpreter death`)
        // Only the (now dead) interpreter writes `done`, so the sequence can
        // never resume after the timeout. (The forked `sleep` may linger
        // briefly as an orphan; it writes nothing and exits on its own.)
        expect(existsSync(doneFile), `under ${shellCase.label}`).toBe(false)
      }, 30_000)
    }
  }
)
