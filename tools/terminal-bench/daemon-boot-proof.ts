// Decisive production-path proof: fork the BUILT, bundled daemon
// (out/main/daemon-entry.js) the same way the Electron app does, with
// ORCA_RUST_TERMINAL=1, then drive it with Orca's own production DaemonClient to
// create a real PTY terminal and snapshot it. If the daemon logs the Rust-engine
// selection and the snapshot shows our live shell output, the Rust terminal
// engine is genuinely live inside the shipping app's daemon.
import { fork } from 'node:child_process'
import { mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { DaemonClient } from '../../src/main/daemon/client'
import type { TerminalSnapshot } from '../../src/main/daemon/types'

const repo = resolve(import.meta.dirname, '..', '..')
const daemonEntry = join(repo, 'out', 'main', 'daemon-entry.js')
const addon = join(repo, 'native', 'orca-node', 'orca_node.node')
const dir = mkdtempSync(join(tmpdir(), 'orca-daemon-'))
const socketPath = join(dir, 'sock')
const tokenPath = join(dir, 'token')

const daemonLog: string[] = []

function startDaemon(): Promise<void> {
  const child = fork(daemonEntry, ['--socket', socketPath, '--token', tokenPath], {
    cwd: repo, // so the addon loader's process.cwd() path resolves
    env: { ...process.env, ORCA_RUST_TERMINAL: '1', ORCA_RUST_TERMINAL_ADDON: addon },
    stdio: ['ignore', 'pipe', 'pipe', 'ipc']
  })
  child.stdout?.on('data', (d) => daemonLog.push(String(d)))
  child.stderr?.on('data', (d) => daemonLog.push(String(d)))
  return new Promise((res, rej) => {
    const t = setTimeout(() => rej(new Error('daemon did not signal ready')), 10000)
    child.on('message', (m: { type?: string }) => {
      if (m?.type === 'ready') {
        clearTimeout(t)
        res()
      }
    })
    child.on('exit', (c) => rej(new Error(`daemon exited early: ${c}`)))
  })
}

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms))

async function main() {
  await startDaemon()
  const client = new DaemonClient({ socketPath, tokenPath })
  await client.ensureConnected()

  const sessionId = 'proof-session'
  const created = await client.request('createOrAttach', {
    sessionId,
    cols: 100,
    rows: 30,
    cwd: repo,
    env: { TERM: 'xterm-256color' }
  })
  console.log('createOrAttach ->', JSON.stringify(created))
  await wait(1500) // shell startup
  await client.request('write', { sessionId, data: 'printf "RUSTPROOF_%s\\n" LIVE_OK\n' })
  await wait(1500)
  const snapResp = (await client.request('getSnapshot', { sessionId })) as {
    snapshot: TerminalSnapshot | null
  }
  const snap = snapResp?.snapshot ?? null
  console.log('snapshot cwd/rows/scrollback ->', snap?.cwd, snap?.rows, snap?.scrollbackLines)

  const log = daemonLog.join('')
  const rustSelected = log.includes('terminal engine: Rust')
  const fellBack = log.includes('did not load')
  const snapText = snap?.snapshotAnsi ?? ''
  const sawOutput = snapText.includes('RUSTPROOF_LIVE_OK')

  console.log('--- daemon log ---')
  console.log(log.trim() || '(none)')
  console.log('--- verdict ---')
  console.log(
    `Rust engine selected in daemon : ${rustSelected ? '✅' : fellBack ? '❌ fell back to TS' : '❓ no log'}`
  )
  console.log(
    `live shell output in snapshot   : ${sawOutput ? '✅ (RUSTPROOF_LIVE_OK present)' : '❌'}`
  )
  console.log(`snapshot bytes                  : ${snapText.length}`)

  try {
    await client.request('shutdown', {})
  } catch {
    /* daemon may close the socket first */
  }
  process.exit(rustSelected && sawOutput ? 0 : 1)
}

main().catch((e) => {
  console.error('PROOF FAILED:', e)
  console.error(`--- daemon log ---\n${daemonLog.join('')}`)
  process.exit(1)
})
