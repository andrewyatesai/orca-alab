// Like session-live-harness but also fires subprocess.resize() mid-stream so we
// can test DECSC/DECRC saved-cursor behavior across a real PTY/emulator resize.
// Env: SCENARIO_CMD, SCENARIO_ARGS(JSON), COLS, ROWS, NEWCOLS, NEWROWS,
// RESIZE_AT_MS, DURATION_MS, OUT.
import { spawn } from 'node-pty'
import { writeFileSync } from 'node:fs'
import { Session, type SubprocessHandle } from '../../src/main/daemon/session'

const cmd = process.env.SCENARIO_CMD ?? '/bin/cat'
const args: string[] = process.env.SCENARIO_ARGS ? JSON.parse(process.env.SCENARIO_ARGS) : []
const cols = Number(process.env.COLS ?? 80)
const rows = Number(process.env.ROWS ?? 24)
const newCols = Number(process.env.NEWCOLS ?? cols)
const newRows = Number(process.env.NEWROWS ?? rows)
const resizeAtMs = Number(process.env.RESIZE_AT_MS ?? 400)
const durationMs = Number(process.env.DURATION_MS ?? 900)
const outPath = process.env.OUT ?? '/tmp/orca-bench/resize.json'

const rawChunks: Buffer[] = []
const dataCbs: ((d: string) => void)[] = []
const exitCbs: ((c: number) => void)[] = []
const resizeEvents: { atMs: number; cols: number; rows: number }[] = []

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
      /* gone */
    }
  },
  signal: (s) => {
    try {
      process.kill(term.pid, s as NodeJS.Signals)
    } catch {
      /* gone */
    }
  },
  onData: (cb) => dataCbs.push(cb),
  onExit: (cb) => exitCbs.push(cb),
  dispose: () => {
    try {
      term.kill()
    } catch {
      /* gone */
    }
  }
}

const session = new Session({
  sessionId: 'resize',
  cols,
  rows,
  subprocess,
  shellReadySupported: false
})

setTimeout(() => {
  resizeEvents.push({ atMs: resizeAtMs, cols: newCols, rows: newRows })
  session.resize(newCols, newRows)
}, resizeAtMs)

setTimeout(() => {
  const snap = session.getSnapshot()
  const raw = Buffer.concat(rawChunks)
  writeFileSync(
    outPath,
    JSON.stringify({
      engine: process.env.ORCA_RUST_TERMINAL === '1' ? 'rust' : 'ts',
      cmd,
      args,
      cols: newCols,
      rows: newRows,
      startCols: cols,
      startRows: rows,
      resizeEvents,
      rawBytes: raw.length,
      rawB64: raw.toString('base64'),
      snapshotAnsi: snap?.snapshotAnsi ?? '',
      modes: snap?.modes ?? null,
      scrollbackLines: snap?.scrollbackLines ?? 0,
      foreground: subprocess.getForegroundProcess()
    })
  )
  session.dispose()
  process.exit(0)
}, durationMs)
