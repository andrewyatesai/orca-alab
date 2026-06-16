// Boots the REAL daemon Session (src/main/daemon/session.ts) against a REAL
// node-pty shell, under whichever terminal engine ORCA_RUST_TERMINAL selects.
// This is the actual app code path: PTY bytes -> Session.handleSubprocessData
// -> emulator.write -> getSnapshot(). We also tee the exact bytes the emulator
// saw so a verifier can check the snapshot against xterm parsing the same bytes.
//
// Driven by env: SCENARIO_CMD, SCENARIO_ARGS(JSON), COLS, ROWS, DURATION_MS,
// INPUTS(JSON [{afterMs,data}]), OUT. ORCA_RUST_TERMINAL + ORCA_RUST_TERMINAL_ADDON
// select/locate the Rust engine.
import { spawn } from 'node-pty'
import { writeFileSync } from 'node:fs'
import { Session, type SubprocessHandle } from '../../src/main/daemon/session'

const cmd = process.env.SCENARIO_CMD ?? '/bin/sh'
const args: string[] = process.env.SCENARIO_ARGS
  ? JSON.parse(process.env.SCENARIO_ARGS)
  : ['-c', 'echo hi']
const cols = Number(process.env.COLS ?? 100)
const rows = Number(process.env.ROWS ?? 30)
const durationMs = Number(process.env.DURATION_MS ?? 1500)
const inputs: { afterMs: number; data: string }[] = process.env.INPUTS
  ? JSON.parse(process.env.INPUTS)
  : []
const outPath = process.env.OUT ?? '/tmp/orca-bench/live.json'

const rawChunks: Buffer[] = []
const dataCbs: ((d: string) => void)[] = []
const exitCbs: ((c: number) => void)[] = []

const term = spawn(cmd, args, {
  cols,
  rows,
  cwd: process.cwd(),
  env: { ...process.env, TERM: 'xterm-256color' } as Record<string, string>
})
term.onData((d) => {
  rawChunks.push(Buffer.from(d, 'utf8'))
  for (const cb of dataCbs) {
    cb(d)
  }
})
term.onExit(({ exitCode }) => {
  for (const cb of exitCbs) {
    cb(exitCode ?? 0)
  }
})

const subprocess: SubprocessHandle = {
  pid: term.pid,
  getForegroundProcess: () => (term as unknown as { process?: string }).process ?? null,
  write: (d) => term.write(d),
  resize: (c, r) => term.resize(c, r),
  kill: () => term.kill(),
  forceKill: () => {
    try {
      process.kill(term.pid, 'SIGKILL')
    } catch {
      /* already gone */
    }
  },
  signal: (s) => {
    try {
      process.kill(term.pid, s as NodeJS.Signals)
    } catch {
      /* already gone */
    }
  },
  onData: (cb) => dataCbs.push(cb),
  onExit: (cb) => exitCbs.push(cb),
  dispose: () => {
    try {
      term.kill()
    } catch {
      /* already gone */
    }
  }
}

const session = new Session({
  sessionId: 'live',
  cols,
  rows,
  subprocess,
  shellReadySupported: false
})

for (const inp of inputs) {
  setTimeout(() => session.write(inp.data), inp.afterMs)
}

setTimeout(() => {
  const snap = session.getSnapshot()
  const raw = Buffer.concat(rawChunks)
  writeFileSync(
    outPath,
    JSON.stringify({
      engine: process.env.ORCA_RUST_TERMINAL === '1' ? 'rust' : 'ts',
      cmd,
      args,
      cols,
      rows,
      rawBytes: raw.length,
      rawB64: raw.toString('base64'),
      snapshotAnsi: snap?.snapshotAnsi ?? '',
      modes: snap?.modes ?? null,
      cwd: snap?.cwd ?? null,
      scrollbackLines: snap?.scrollbackLines ?? 0,
      foreground: subprocess.getForegroundProcess()
    })
  )
  session.dispose()
  process.exit(0)
}, durationMs)
