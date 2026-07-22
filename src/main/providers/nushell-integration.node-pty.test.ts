/**
 * Real-binary integration for the nu shell-ready launch config (#8928 PR1,
 * design §11) — mirrors omp-shell-wrapper.node-pty.test.ts. Skips when `nu`
 * is absent or below the integration floor; CI provides the version matrix.
 */
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { nushellVersionSupportsIntegration } from '../pty/nushell-capability-probe'

const describePosix = process.platform === 'win32' ? describe.skip : describe

function resolveNuBinary(): string | null {
  if (process.platform === 'win32') {
    return null
  }
  const which = spawnSync('which', ['nu'], { encoding: 'utf8' })
  const nuPath = which.status === 0 ? which.stdout.trim().split('\n')[0] : null
  if (!nuPath) {
    return null
  }
  const version = spawnSync(nuPath, ['--version'], { encoding: 'utf8' })
  if (version.status !== 0 || !nushellVersionSupportsIntegration(version.stdout)) {
    return null
  }
  return nuPath
}

const NU_PATH = resolveNuBinary()
const itWithNu = NU_PATH ? it : it.skip

const SHELL_READY_MARKER = '\x1b]777;orca-shell-ready\x07'

const tempDirs: string[] = []

function makeTempDir(prefix: string): string {
  const dir = mkdtempSync(join(tmpdir(), prefix))
  tempDirs.push(dir)
  return dir
}

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true })
  }
  delete process.env.ORCA_USER_DATA_PATH
})

type NuReplRun = {
  output: string
  markerCount: number
}

async function runNuRepl(args: {
  nuPath: string
  extraEnv: Record<string, string>
  commands: string[]
  doneToken: string
}): Promise<NuReplRun> {
  vi.resetModules()
  const probe = await import('../pty/nushell-capability-probe')
  const shellReady = await import('./local-pty-shell-ready')
  await probe.probeNushellIntegrationSupport(args.nuPath)
  expect(probe.getCachedNushellIntegrationSupport(args.nuPath)).toBe(true)
  const config = shellReady.getShellReadyLaunchConfig(args.nuPath)
  expect(config.args?.[0]).toBe('-l')
  expect(config.args?.[1]).toBe('-e')

  const pty = await import('node-pty')
  const home = makeTempDir('orca-nu-home-')
  // Why: point nu at an empty config dir so user config cannot alter marker/OSC behavior.
  mkdirSync(join(home, '.config'), { recursive: true })
  const proc = pty.spawn(args.nuPath, config.args ?? [], {
    name: 'xterm-256color',
    cols: 120,
    rows: 30,
    cwd: home,
    env: {
      PATH: process.env.PATH ?? '/usr/bin:/bin',
      HOME: home,
      XDG_CONFIG_HOME: join(home, '.config'),
      TERM: 'xterm-256color',
      ...config.env,
      ...args.extraEnv
    }
  })

  let output = ''
  let commandIndex = 0
  let markerSeen = false
  const done = new Promise<void>((resolve, reject) => {
    const deadline = setTimeout(
      () => reject(new Error(`timed out waiting for nu PTY output:\n${output}`)),
      20_000
    )
    const finish = (): void => {
      clearTimeout(deadline)
      resolve()
    }
    proc.onData((chunk) => {
      output += chunk
      if (!markerSeen && output.includes(SHELL_READY_MARKER)) {
        markerSeen = true
        proc.write(`${args.commands[commandIndex]}\r`)
        return
      }
      if (markerSeen && commandIndex < args.commands.length - 1) {
        // Advance when the previous command's sentinel output landed.
        if (output.includes(`done-${commandIndex}`)) {
          commandIndex++
          proc.write(`${args.commands[commandIndex]}\r`)
        }
        return
      }
      if (output.includes(args.doneToken)) {
        finish()
      }
    })
    proc.onExit(() => finish())
  })
  try {
    await done
  } finally {
    proc.kill()
  }
  return { output, markerCount: output.split(SHELL_READY_MARKER).length - 1 }
}

describePosix('nushell shell-ready integration (real nu)', () => {
  itWithNu(
    'emits the OSC 777 marker once, forces OSC 133, and keeps -e mutations in the REPL',
    async () => {
      const userData = makeTempDir('orca-nu-userdata-')
      process.env.ORCA_USER_DATA_PATH = userData
      const shimDir = makeTempDir('orca-nu-shim-')
      const opencodeDir = join(makeTempDir('orca-nu-opencode-'), 'config')

      const run = await runNuRepl({
        nuPath: NU_PATH!,
        extraEnv: {
          ORCA_ATTRIBUTION_SHIM_DIR: shimDir,
          ORCA_OPENCODE_CONFIG_DIR: opencodeDir
        },
        commands: [
          // (e) attribution shim dir is PATH-front after startup (integration ran post rc-files).
          'print $"PATHFRONT=($env.PATH | first) done-0"',
          // (c) -e env mutations survive into the REPL.
          'print $"OPENCODE=($env.OPENCODE_CONFIG_DIR? | default missing) done-1"',
          // (b) a real command between prompts so 133;C/D fire; then a second prompt to prove the marker once-guard.
          'print "proof-command-ran ALL-DONE"'
        ],
        doneToken: 'ALL-DONE'
      })

      // (a) exactly one BEL-terminated OSC 777 marker across multiple prompts (string-hook once-guard).
      expect(run.markerCount).toBe(1)
      expect(run.output).toContain(`PATHFRONT=${shimDir}`)
      expect(run.output).toContain(`OPENCODE=${opencodeDir}`)
      expect(run.output).toContain('proof-command-ran')
      // (b) nu's native shell integration is force-enabled: prompt marks and command lifecycle marks present.
      expect(run.output).toContain(']133;A')
      expect(run.output).toContain(']133;C')
      expect(run.output).toContain(']133;D')
    },
    30_000
  )

  itWithNu(
    'delivers a startup command exactly once after the marker (stdin path)',
    async () => {
      const userData = makeTempDir('orca-nu-userdata-')
      process.env.ORCA_USER_DATA_PATH = userData

      const run = await runNuRepl({
        nuPath: NU_PATH!,
        extraEnv: {},
        commands: ['print "startup-proof STARTUP-DONE"'],
        doneToken: 'STARTUP-DONE'
      })

      expect(run.markerCount).toBe(1)
      // The command executed once: one echoed input line + one printed result.
      const executions = run.output.split('startup-proof STARTUP-DONE').length - 1
      expect(executions).toBeGreaterThanOrEqual(1)
      expect(executions).toBeLessThanOrEqual(2)
    },
    30_000
  )
})
