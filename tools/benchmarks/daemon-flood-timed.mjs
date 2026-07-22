#!/usr/bin/env node
// The daemon flood harness (`daemon-flood-timed` class) — the receiver-timed
// end-to-end measurement from docs/rust-migration/daemon-pty-drain-investigation.md,
// restored from scratchpad as a committed tool: launch the REAL `orca-daemon`
// binary, attach a control+stream client pair (NDJSON or v1020 binary), flood
// `cat <corpus>` through a session, and time createOrAttach → exit event as
// consumed by the client. This measures the full production path — PTY read →
// decode → engine feed → route_output → stream writer (P2 coalescing) → socket
// → client — where pump_bench.rs stops at read+engine and stream_flood_bench.rs
// runs serve() in-process.
//
// Modes:
//   --mode native         client on the daemon's local socket (default)
//   --mode ssh-localhost  client consumes through `ssh -L` Unix-socket
//                         forwarding, so every stream frame crosses a real SSH
//                         channel (the writer-under-SSH measurement the P2
//                         investigation left unclaimed)
//
// Examples (see the investigation doc's Reproduce section):
//   node tools/benchmarks/daemon-flood-timed.mjs --mb 500 --trials 5
//   node tools/benchmarks/daemon-flood-timed.mjs --binary --mode ssh-localhost
//   node tools/benchmarks/daemon-flood-timed.mjs --daemon-bin <before-bin> --label baseline
//
// Run on a QUIET machine and interleave before/after binaries (ABBA) via
// --daemon-bin/--label; loadavg is printed with the summary for the record.

import { spawn, spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, rmSync } from 'node:fs'
import net from 'node:net'
import os from 'node:os'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { defaultCorpusPath, ensureFloodCorpus } from './daemon-flood-corpus.mjs'
import {
  createOrAttachLine,
  helloLine,
  makeExitEventScanner,
  sshForwardArgs,
  sshPreflightArgs,
  summarizeRates
} from './daemon-flood-wire.mjs'

const USAGE = `usage: node tools/benchmarks/daemon-flood-timed.mjs [options]
  --mode native|ssh-localhost   transport for the client sockets (default native)
  --binary                      v1020 binary stream format (default ndjson)
  --mb N                        corpus size in MB (default 200)
  --corpus <path>               corpus file; generated deterministically if missing
  --trials N                    timed flood repetitions (default 5)
  --daemon-bin <path>           orca-daemon binary (default rust/target/{release,debug})
  --ssh-dest <dest>             ssh destination for ssh-localhost mode (default localhost)
  --ssh-arg <arg>               extra ssh arg, repeatable (e.g. -p/-i/-o for a scratch sshd)
  --label <s>                   tag for output rows (ABBA bookkeeping)
  --timeout-secs N              per-trial watchdog (default 300)`

export function parseFloodArgs(argv) {
  const opts = {
    mode: 'native',
    binary: false,
    mb: 200,
    corpus: null,
    trials: 5,
    daemonBin: null,
    sshDest: 'localhost',
    sshArgs: [],
    label: null,
    timeoutSecs: 300
  }
  const takeValue = (flag, i) => {
    if (i + 1 >= argv.length) {
      throw new Error(`${flag} requires a value\n${USAGE}`)
    }
    return argv[i + 1]
  }
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]
    switch (a) {
      case '--mode':
        opts.mode = takeValue(a, i++)
        break
      case '--binary':
        opts.binary = true
        break
      case '--mb':
        opts.mb = Number(takeValue(a, i++))
        break
      case '--corpus':
        opts.corpus = takeValue(a, i++)
        break
      case '--trials':
        opts.trials = Number(takeValue(a, i++))
        break
      case '--daemon-bin':
        opts.daemonBin = takeValue(a, i++)
        break
      case '--ssh-dest':
        opts.sshDest = takeValue(a, i++)
        break
      case '--ssh-arg':
        opts.sshArgs.push(takeValue(a, i++))
        break
      case '--label':
        opts.label = takeValue(a, i++)
        break
      case '--timeout-secs':
        opts.timeoutSecs = Number(takeValue(a, i++))
        break
      case '--help':
        throw new Error(USAGE)
      default:
        throw new Error(`unknown arg ${a}\n${USAGE}`)
    }
  }
  if (opts.mode !== 'native' && opts.mode !== 'ssh-localhost') {
    throw new Error(`--mode must be native or ssh-localhost, got ${opts.mode}\n${USAGE}`)
  }
  for (const [flag, v] of [
    ['--mb', opts.mb],
    ['--trials', opts.trials],
    ['--timeout-secs', opts.timeoutSecs]
  ]) {
    if (!Number.isFinite(v) || v <= 0) {
      throw new Error(`${flag} must be a positive number\n${USAGE}`)
    }
  }
  return opts
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

function withTimeout(promise, ms, what) {
  let timer
  return Promise.race([
    promise.finally(() => clearTimeout(timer)),
    new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(`timed out after ${ms / 1000}s: ${what}`)), ms)
    })
  ])
}

function resolveDaemonBin(explicit) {
  if (explicit) {
    if (!existsSync(explicit)) {
      throw new Error(`--daemon-bin not found: ${explicit}`)
    }
    return explicit
  }
  const repoRoot = resolve(import.meta.dirname, '..', '..')
  const exe = process.platform === 'win32' ? 'orca-daemon.exe' : 'orca-daemon'
  // Why: release first — debug-build throughput is not a usable flood number.
  const candidates = ['release', 'debug'].map((p) => join(repoRoot, 'rust', 'target', p, exe))
  const bin = candidates.find((p) => existsSync(p))
  if (!bin) {
    throw new Error(
      `orca-daemon binary not built (looked at ${candidates.join(', ')}) — run:\n` +
        '  cargo build --release -p orca-daemon --manifest-path rust/Cargo.toml'
    )
  }
  if (bin === candidates[1]) {
    console.warn('WARNING: using a DEBUG orca-daemon build — numbers will be far below release.')
  }
  return bin
}

function connectWithRetry(path, { tries = 240, delayMs = 25, aborted = () => null } = {}) {
  return new Promise((resolvePromise, reject) => {
    let attempt = 0
    const tryOnce = () => {
      const abortReason = aborted()
      if (abortReason) {
        reject(new Error(abortReason))
        return
      }
      const socket = net.createConnection(path)
      socket.once('connect', () => resolvePromise(socket))
      socket.once('error', () => {
        socket.destroy()
        attempt += 1
        if (attempt >= tries) {
          reject(new Error(`could not connect to ${path} after ${tries} attempts`))
        } else {
          setTimeout(tryOnce, delayMs)
        }
      })
    }
    tryOnce()
  })
}

// Send the hello line, await its NDJSON reply; any bytes past the reply's
// newline (possible on the stream socket) are returned for the drain.
function helloHandshake(socket, line, what) {
  return withTimeout(
    new Promise((resolvePromise, reject) => {
      let buffered = Buffer.alloc(0)
      const onData = (chunk) => {
        buffered = Buffer.concat([buffered, chunk])
        const nl = buffered.indexOf(0x0a)
        if (nl === -1) {
          return
        }
        socket.removeListener('data', onData)
        try {
          const reply = JSON.parse(buffered.subarray(0, nl).toString('utf8'))
          if (reply.ok !== true) {
            reject(new Error(`${what} hello rejected: ${reply.error ?? 'unknown'}`))
            return
          }
          resolvePromise(buffered.subarray(nl + 1))
        } catch (err) {
          reject(new Error(`${what} hello reply unparsable: ${err}`))
        }
      }
      socket.on('data', onData)
      socket.once('error', reject)
      socket.write(line)
    }),
    8000,
    `${what} hello`
  )
}

// Control-socket RPC responses, id-correlated (NDJSON both stream formats).
function makeControlReader(socket) {
  const pending = new Map()
  let buffer = ''
  socket.on('data', (chunk) => {
    buffer += chunk.toString('utf8')
    let nl = buffer.indexOf('\n')
    while (nl !== -1) {
      const line = buffer.slice(0, nl)
      buffer = buffer.slice(nl + 1)
      nl = buffer.indexOf('\n')
      if (line.trim()) {
        const obj = JSON.parse(line)
        const entry = obj.id ? pending.get(obj.id) : undefined
        if (entry) {
          pending.delete(obj.id)
          entry(obj)
        }
      }
    }
  })
  return {
    expect(id) {
      return new Promise((resolvePromise) => pending.set(id, resolvePromise))
    }
  }
}

// Raw stream drain: one persistent data handler, one scanner per trial. Bytes
// arriving with no active trial (post-exit stragglers) are discarded so they
// can never leak into the next trial's wire count.
function makeStreamDrain(socket, helloRemainder) {
  let active = null
  let closed = false
  let pendingRemainder = helloRemainder?.length ? helloRemainder : null
  socket.on('data', (chunk) => {
    if (active && active.scanner.push(chunk)) {
      const finished = active
      active = null
      finished.resolve()
    }
  })
  socket.on('close', () => {
    closed = true
    if (active) {
      const finished = active
      active = null
      finished.reject(new Error('stream socket closed before exit event'))
    }
  })
  return {
    begin(scanner) {
      if (closed) {
        return Promise.reject(new Error('stream socket already closed'))
      }
      return new Promise((resolvePromise, reject) => {
        active = { scanner, resolve: resolvePromise, reject }
        if (pendingRemainder) {
          const r = pendingRemainder
          pendingRemainder = null
          if (scanner.push(r)) {
            active = null
            resolvePromise()
          }
        }
      })
    }
  }
}

async function main() {
  const opts = parseFloodArgs(process.argv.slice(2))
  if (opts.mode === 'ssh-localhost' && process.platform === 'win32') {
    throw new Error(
      'ssh-localhost mode needs OpenSSH Unix-socket forwarding, unavailable on win32 — ' +
        'run this mode from a unix host (or inside WSL) instead.'
    )
  }
  const daemonBin = resolveDaemonBin(opts.daemonBin)
  const corpusPath = opts.corpus ?? defaultCorpusPath(opts.mb)
  const corpusBytes = await ensureFloodCorpus(corpusPath, opts.mb)
  const corpusMb = corpusBytes / 1e6

  const cleanup = []
  const scratch = process.platform === 'win32' ? null : mkdtempSync(join(os.tmpdir(), 'daemon-flood-'))
  if (scratch) {
    cleanup.push(() => rmSync(scratch, { recursive: true, force: true }))
  }
  const daemonSocket =
    process.platform === 'win32'
      ? `\\\\?\\pipe\\orca-daemon-flood-${process.pid}-${Date.now()}`
      : join(scratch, 'daemon.sock')

  try {
    // ── real daemon binary, token-less (parity-harness mode) ──
    let daemonExit = null
    const daemon = spawn(daemonBin, [daemonSocket], { stdio: ['ignore', 'ignore', 'inherit'] })
    daemon.on('exit', (code, sig) => (daemonExit = sig ?? code))
    cleanup.push(() => daemon.kill('SIGKILL'))
    const daemonAborted = () => (daemonExit !== null ? `daemon exited early (${daemonExit})` : null)
    ;(await connectWithRetry(daemonSocket, { aborted: daemonAborted })).destroy()

    // ── optional ssh tunnel: client consumes through a real SSH channel ──
    let clientSocket = daemonSocket
    if (opts.mode === 'ssh-localhost') {
      const preflight = spawnSync(
        'ssh',
        sshPreflightArgs({ destination: opts.sshDest, extraSshArgs: opts.sshArgs }),
        { encoding: 'utf8', timeout: 15000 }
      )
      if (preflight.status !== 0) {
        throw new Error(
          `ssh preflight to '${opts.sshDest}' failed (needs non-interactive key auth):\n` +
            `${(preflight.stderr ?? '').trim()}\n` +
            'Point --ssh-dest/--ssh-arg at a reachable sshd (port, identity, known-hosts).'
        )
      }
      clientSocket = join(scratch, 'ssh-fwd.sock')
      let sshExit = null
      let sshStderr = ''
      const tunnel = spawn(
        'ssh',
        sshForwardArgs({
          destination: opts.sshDest,
          localSocket: clientSocket,
          remoteSocket: daemonSocket,
          extraSshArgs: opts.sshArgs
        }),
        { stdio: ['ignore', 'ignore', 'pipe'] }
      )
      tunnel.stderr.setEncoding('utf8')
      tunnel.stderr.on('data', (d) => (sshStderr += d))
      tunnel.on('exit', (code, sig) => (sshExit = sig ?? code))
      cleanup.push(() => tunnel.kill('SIGKILL'))
      const tunnelAborted = () =>
        sshExit !== null ? `ssh tunnel exited early (${sshExit}): ${sshStderr.trim()}` : null
      ;(await connectWithRetry(clientSocket, { aborted: tunnelAborted })).destroy()
    }

    // ── control + stream client pair (control first, matching client.ts) ──
    const control = await connectWithRetry(clientSocket)
    cleanup.push(() => control.destroy())
    await helloHandshake(control, helloLine('control'), 'control')
    const controlReader = makeControlReader(control)

    const stream = await connectWithRetry(clientSocket)
    cleanup.push(() => stream.destroy())
    const streamRemainder = await helloHandshake(
      stream,
      helloLine('stream', { binaryStream: opts.binary }),
      'stream'
    )
    const drain = makeStreamDrain(stream, streamRemainder)

    const label = opts.label ? `[${opts.label}] ` : ''
    const format = opts.binary ? 'binary' : 'ndjson'
    console.log(
      `${label}daemon-flood-timed mode=${opts.mode} format=${format} corpus=${corpusPath} ` +
        `(${corpusMb.toFixed(1)} MB) daemon=${daemonBin}`
    )

    // ── timed trials: createOrAttach → exit event, receiver-side ──
    const corpusRates = []
    const wireRates = []
    for (let t = 1; t <= opts.trials; t++) {
      const sessionId = `flood-${process.pid}-${t}`
      const rpcId = `c${t}`
      const scanner = makeExitEventScanner()
      const consumed = drain.begin(scanner)
      const rpcReply = controlReader.expect(rpcId)
      const t0 = performance.now()
      control.write(createOrAttachLine({ id: rpcId, sessionId, corpusPath }))
      await withTimeout(consumed, opts.timeoutSecs * 1000, `trial ${t} flood`)
      const secs = (performance.now() - t0) / 1000
      const reply = await withTimeout(rpcReply, 8000, `trial ${t} createOrAttach reply`)
      if (reply.ok !== true) {
        throw new Error(`createOrAttach failed: ${JSON.stringify(reply)}`)
      }
      // Why: every format inflates the corpus (\n→\r\n via OPOST, framing/escapes),
      // so wire < corpus means the stream ended short — a broken run, not a fast one.
      if (scanner.wireBytes < corpusBytes) {
        throw new Error(
          `trial ${t}: wire bytes ${scanner.wireBytes} < corpus ${corpusBytes} — truncated stream`
        )
      }
      corpusRates.push(corpusMb / secs)
      wireRates.push(scanner.wireBytes / 1e6 / secs)
      console.log(
        `${label}trial ${t}/${opts.trials}  elapsed ${secs.toFixed(3)}s  ` +
          `corpus ${(corpusMb / secs).toFixed(1)} MB/s  wire ${(scanner.wireBytes / 1e6 / secs).toFixed(1)} MB/s`
      )
      // Why: give post-exit stragglers a beat to land (discarded) before the next trial.
      await sleep(150)
    }

    const c = summarizeRates(corpusRates)
    const w = summarizeRates(wireRates)
    console.log(
      `${label}summary mode=${opts.mode} format=${format} trials=${opts.trials}  ` +
        `corpus MB/s median=${c.median.toFixed(1)} mean=${c.mean.toFixed(1)} ` +
        `min=${c.min.toFixed(1)} max=${c.max.toFixed(1)}  ` +
        `wire MB/s median=${w.median.toFixed(1)}  ` +
        `loadavg=${os
          .loadavg()
          .map((v) => v.toFixed(1))
          .join(',')} host=${process.platform}/${os.arch()}`
    )
  } finally {
    for (const fn of cleanup.toReversed()) {
      try {
        fn()
      } catch {
        /* best-effort */
      }
    }
  }
}

const invokedDirectly =
  process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href
if (invokedDirectly) {
  main().catch((err) => {
    console.error(err.message ?? err)
    process.exitCode = 1
  })
}
