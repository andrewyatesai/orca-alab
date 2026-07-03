#!/usr/bin/env node
// Daemon parity gate. Drives ONE stateful RPC corpus (request-vectors.mjs) over
// the real Unix-socket transport against both daemons and compares the
// volatile-free structural fingerprints:
//
//   Leg A (hard gate)  — the Rust `orca-daemon`: must satisfy the behavioral
//                        invariants below. This is the first coverage of the
//                        socket path itself (hello handshake, control+stream
//                        pairing, NDJSON framing, event delivery) — the
//                        in-process rpc_lifecycle.rs tests bypass all of it.
//   Leg B (differential) — the Node daemon (out/main/daemon-entry.js via
//                        electron-as-node): best-effort. If it comes up, its
//                        fingerprint is diffed against Rust's and any
//                        divergence FAILS the gate. If it cannot be spawned in
//                        this environment, the leg is loudly SKIPPED (not
//                        silently passed) — the Rust invariants still gate.
//
// Usage: node tools/daemon-parity/run.mjs

import { spawn } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs'
import os from 'node:os'
import { join, resolve } from 'node:path'
import { DaemonSocketClient } from './daemon-socket-client.mjs'
import { driveDaemon, parityConstants } from './request-vectors.mjs'

const PROTOCOL_VERSION = 18
const repoRoot = resolve(import.meta.dirname, '..', '..')
const scratch = mkdtempSync(join(os.tmpdir(), 'daemon-parity-'))
const cleanup = []
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

async function waitFor(pred, { tries = 200, delayMs = 25 } = {}) {
  for (let i = 0; i < tries; i++) {
    if (pred()) {
      return true
    }
    await sleep(delayMs)
  }
  return false
}

async function connectWithRetry(client, socketPath, tries = 200) {
  for (let i = 0; i < tries; i++) {
    try {
      await client.connect(socketPath)
      return
    } catch {
      await sleep(25)
    }
  }
  throw new Error(`could not connect to daemon socket ${socketPath}`)
}

// ── Leg A: the Rust orca-daemon ────────────────────────────────────────────
async function runRustLeg() {
  const bin = ['debug', 'release']
    .map((p) => join(repoRoot, `rust/target/${p}/orca-daemon`))
    .find((p) => existsSync(p))
  if (!bin) {
    throw new Error(
      'orca-daemon binary not built — run:\n' +
        '  PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" \\\n' +
        '    cargo build -p orca-daemon --manifest-path rust/Cargo.toml --offline'
    )
  }
  const socketPath = join(scratch, 'rust.sock')
  const child = spawn(bin, [socketPath], { stdio: ['ignore', 'ignore', 'inherit'] })
  cleanup.push(() => child.kill('SIGKILL'))
  await waitFor(() => existsSync(socketPath))

  const client = new DaemonSocketClient({
    token: 'any-token-rust-accepts',
    protocolVersion: PROTOCOL_VERSION,
    clientId: 'parity-rust'
  })
  await connectWithRetry(client, socketPath)
  const transcript = await driveDaemon(client)
  client.close()
  return transcript
}

// ── Leg B: the Node daemon (best-effort) ────────────────────────────────────
async function runNodeLeg() {
  const entry = join(repoRoot, 'out/main/daemon-entry.js')
  const electron = join(repoRoot, 'node_modules/electron/dist/Electron.app/Contents/MacOS/Electron')
  if (!existsSync(entry)) {
    return { skipped: `no ${entry} — run \`pnpm build:electron-vite\`` }
  }
  if (!existsSync(electron)) {
    return { skipped: `no electron binary at ${electron}` }
  }
  const socketPath = join(scratch, 'node.sock')
  const tokenPath = join(scratch, 'node.token')
  let stderr = ''
  const child = spawn(electron, [entry, '--socket', socketPath, '--token', tokenPath], {
    env: { ...process.env, ELECTRON_RUN_AS_NODE: '1' },
    stdio: ['ignore', 'ignore', 'pipe']
  })
  cleanup.push(() => child.kill('SIGKILL'))
  child.stderr.setEncoding('utf8')
  child.stderr.on('data', (d) => (stderr += d))
  let exited = null
  child.on('exit', (code, sig) => (exited = sig ?? code))

  // The daemon writes its generated token to tokenPath once it is listening.
  const ready = await waitFor(() => existsSync(tokenPath) && existsSync(socketPath))
  if (!ready) {
    const tail = stderr.split('\n').slice(-8).join('\n')
    return { skipped: `Node daemon did not come up (exit=${exited}). stderr tail:\n${tail}` }
  }
  const token = readFileSync(tokenPath, 'utf8').trim()
  const client = new DaemonSocketClient({
    token,
    protocolVersion: PROTOCOL_VERSION,
    clientId: 'parity-node'
  })
  try {
    await connectWithRetry(client, socketPath, 80)
    const transcript = await driveDaemon(client)
    client.close()
    return { transcript }
  } catch (err) {
    return {
      skipped: `Node leg drive failed: ${err.message}\nstderr:\n${stderr.split('\n').slice(-8).join('\n')}`
    }
  }
}

// ── Invariants the Rust leg must satisfy (the hard gate / golden) ────────────
function checkInvariants(transcript) {
  const { CWD } = parityConstants
  const by = Object.fromEntries(transcript.steps.map((s) => [s.step, s.projection]))
  const checks = [
    ['ping pong', by.ping?.ok === true && by.ping?.pong === true],
    [
      'create isNew:true',
      by['createOrAttach:new']?.ok === true && by['createOrAttach:new']?.isNew === true
    ],
    ['create has numeric pid', by['createOrAttach:new']?.pidType === 'number'],
    ['getCwd == OSC-7 cwd', by.getCwd?.ok === true && by.getCwd?.cwd === CWD],
    ['snapshot dims 88x26', by.getSnapshot?.cols === 88 && by.getSnapshot?.rows === 26],
    ['snapshot cwd', by.getSnapshot?.cwd === CWD],
    ['snapshot carries marker', by.getSnapshot?.snapshotHasMarker === true],
    ['getSize 88x26', by.getSize?.cols === 88 && by.getSize?.rows === 26],
    ['listSessions alive', by.listSessions?.found === true && by.listSessions?.isAlive === true],
    ['stream carried marker', by.streamData?.hasMarker === true],
    ['reattach isNew:false', by['createOrAttach:reattach']?.isNew === false],
    ['resize 100x30', by.resize?.cols === 100 && by.resize?.rows === 30],
    ['unknown write errors', by['write:unknown']?.ok === false],
    [
      'unknown snapshot → ok+null',
      by['getSnapshot:unknown']?.ok === true && by['getSnapshot:unknown']?.snapshotIsNull === true
    ],
    ['kill → not alive', by.kill?.ok === true && by.kill?.noLongerAlive === true]
  ]
  return checks.map(([name, pass]) => ({ name, pass }))
}

// ── Structural diff between the two fingerprints ─────────────────────────────
function diffTranscripts(rust, node) {
  const divergences = []
  const nodeBy = Object.fromEntries(node.steps.map((s) => [s.step, s.projection]))
  for (const { step, projection } of rust.steps) {
    const other = nodeBy[step]
    const a = JSON.stringify(projection)
    const b = JSON.stringify(other)
    if (a !== b) {
      divergences.push({ step, rust: projection, node: other ?? null })
    }
  }
  return divergences
}

function fmt(v) {
  return JSON.stringify(v)
}

async function main() {
  let failed = false
  console.log('── Leg A: Rust orca-daemon (hard gate) ──')
  const rust = await runRustLeg()
  const invariants = checkInvariants(rust)
  for (const { name, pass } of invariants) {
    console.log(`  ${pass ? '✓' : '✗'} ${name}`)
    if (!pass) {
      failed = true
    }
  }
  if (failed) {
    console.log('\n  Rust transcript:')
    for (const s of rust.steps) {
      console.log(`    ${s.step}: ${fmt(s.projection)}`)
    }
  }

  console.log('\n── Leg B: Node daemon (differential) ──')
  const node = await runNodeLeg()
  if (node.skipped) {
    console.log(`  ⊘ SKIPPED — ${node.skipped}`)
    console.log('  (differential not run; Rust invariants above are the gate)')
  } else {
    const divergences = diffTranscripts(rust, node.transcript)
    if (divergences.length === 0) {
      console.log(`  ✓ Node == Rust across ${rust.steps.length} steps (structural)`)
    } else {
      failed = true
      console.log(`  ✗ ${divergences.length} divergence(s):`)
      for (const d of divergences) {
        console.log(`    [${d.step}]`)
        console.log(`      rust: ${fmt(d.rust)}`)
        console.log(`      node: ${fmt(d.node)}`)
      }
    }
  }

  console.log(`\n${failed ? '✗ daemon parity FAILED' : '✓ daemon parity PASSED'}`)
  process.exitCode = failed ? 1 : 0
}

main()
  .catch((err) => {
    console.error(err)
    process.exitCode = 1
  })
  .finally(() => {
    for (const fn of cleanup) {
      try {
        fn()
      } catch {
        /* best-effort */
      }
    }
    try {
      rmSync(scratch, { recursive: true, force: true })
    } catch {
      /* best-effort */
    }
  })
