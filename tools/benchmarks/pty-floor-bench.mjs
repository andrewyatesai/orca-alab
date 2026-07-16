#!/usr/bin/env node
// PTY kernel-floor benchmark: measures the raw throughput ceiling of the OS pty
// layer (the app's real first hop, via node-pty) against a plain child_process
// pipe baseline. Produces the first row of the per-hop ingest budget — no
// engine-bound throughput claim is meaningful without this floor.

import { spawn as spawnChildProcess } from 'node:child_process'
import { existsSync, mkdirSync, statSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'

const TRIAL_COUNT = 5
const LINE_WIDTH = 120
const TARGET_FILE_BYTES = 16 * 1024 * 1024
const BYTES_PER_MB = 1024 * 1024
// Quiet window after child exit before a trial is declared complete: excluded
// from timing (elapsed ends at the last data chunk), so it cannot inflate MB/s.
const POST_EXIT_SETTLE_MS = 150
const TRIAL_TIMEOUT_MS = 60_000

const resultsDir = path.join(import.meta.dirname, 'results')

function buildCorpusFile() {
  const filePath = path.join(os.tmpdir(), 'orc-pty-floor-corpus-16mb.txt')
  const bytesPerLine = LINE_WIDTH + 1
  const lineCount = Math.ceil(TARGET_FILE_BYTES / bytesPerLine)
  const fileBytes = lineCount * bytesPerLine
  if (existsSync(filePath) && statSync(filePath).size === fileBytes) {
    return { filePath, fileBytes, lineCount }
  }
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789'
  const filler = alphabet.repeat(Math.ceil(LINE_WIDTH / alphabet.length) + 1)
  const lines = []
  for (let i = 0; i < lineCount; i++) {
    const prefix = `${String(i).padStart(8, '0')} `
    // Rotate the filler by line index so content is deterministic but non-uniform.
    const body = filler.slice(
      i % alphabet.length,
      (i % alphabet.length) + LINE_WIDTH - prefix.length
    )
    lines.push(`${prefix}${body}\n`)
  }
  writeFileSync(filePath, lines.join(''), 'ascii')
  return { filePath, fileBytes, lineCount }
}

function childCatCommand(filePath) {
  if (process.platform === 'win32') {
    return { command: 'cmd', args: ['/c', 'type', filePath] }
  }
  return { command: 'cat', args: [filePath] }
}

function median(values) {
  const sorted = [...values].sort((a, b) => a - b)
  const mid = Math.floor(sorted.length / 2)
  if (sorted.length % 2 === 1) {
    return sorted[mid]
  }
  return (sorted[mid - 1] + sorted[mid]) / 2
}

function trialThroughput(trial) {
  return trial.receivedChars / BYTES_PER_MB / (trial.elapsedMs / 1000)
}

function runPtyTrial(ptyModule, filePath, fileBytes) {
  const { command, args } = childCatCommand(filePath)
  return new Promise((resolve, reject) => {
    const startedAt = performance.now()
    const proc = ptyModule.spawn(command, args, {
      // Mirror the app's real spawn shape (local-pty-provider) so the measured
      // hop is the one production traffic crosses.
      name: 'xterm-256color',
      cols: 200,
      rows: 50,
      cwd: os.tmpdir(),
      env: process.env
    })
    let receivedChars = 0
    let firstChunkAt = null
    let lastChunkAt = startedAt
    let exited = false
    let settleTimer = null
    let done = false

    const timeoutTimer = setTimeout(() => {
      if (done) {
        return
      }
      done = true
      try {
        proc.kill()
      } catch {
        /* Child may already be gone; the timeout diagnostic below is what matters. */
      }
      reject(
        new Error(
          `pty trial timed out after ${TRIAL_TIMEOUT_MS}ms: received ${receivedChars}/${fileBytes} chars, exited=${exited}`
        )
      )
    }, TRIAL_TIMEOUT_MS)

    const finish = () => {
      if (done) {
        return
      }
      done = true
      clearTimeout(timeoutTimer)
      resolve({
        elapsedMs: lastChunkAt - startedAt,
        firstChunkMs: firstChunkAt === null ? null : firstChunkAt - startedAt,
        receivedChars,
        payloadShortfall: receivedChars < fileBytes
      })
    }

    const maybeScheduleFinish = () => {
      // Completion keys off child exit + quiescence rather than an exact byte
      // target: the pty line discipline rewrites \n to \r\n (ONLCR) on POSIX, so
      // the delivered char count is termios-dependent and an exact match could hang.
      if (!exited || receivedChars < fileBytes) {
        return
      }
      if (settleTimer !== null) {
        clearTimeout(settleTimer)
      }
      settleTimer = setTimeout(finish, POST_EXIT_SETTLE_MS)
    }

    proc.onData((data) => {
      const now = performance.now()
      if (firstChunkAt === null) {
        firstChunkAt = now
      }
      lastChunkAt = now
      receivedChars += data.length
      maybeScheduleFinish()
    })
    proc.onExit(() => {
      exited = true
      maybeScheduleFinish()
    })
    // Drain at max rate: flow control stays off and resume() guards against any
    // spawn path that starts paused — backpressure must land on the child
    // writer (that IS the ceiling under test), never on our reader.
    if (typeof proc.resume === 'function') {
      proc.resume()
    }
  })
}

function runPipeTrial(filePath, fileBytes) {
  const { command, args } = childCatCommand(filePath)
  return new Promise((resolve, reject) => {
    const startedAt = performance.now()
    const child = spawnChildProcess(command, args, { stdio: ['ignore', 'pipe', 'inherit'] })
    let receivedChars = 0
    let firstChunkAt = null
    let lastChunkAt = startedAt

    const timeoutTimer = setTimeout(() => {
      child.kill()
      reject(
        new Error(
          `pipe trial timed out after ${TRIAL_TIMEOUT_MS}ms: received ${receivedChars}/${fileBytes} bytes`
        )
      )
    }, TRIAL_TIMEOUT_MS)

    child.on('error', (error) => {
      clearTimeout(timeoutTimer)
      reject(error)
    })
    child.stdout.on('data', (chunk) => {
      const now = performance.now()
      if (firstChunkAt === null) {
        firstChunkAt = now
      }
      lastChunkAt = now
      receivedChars += chunk.length
    })
    child.on('close', () => {
      clearTimeout(timeoutTimer)
      resolve({
        elapsedMs: lastChunkAt - startedAt,
        firstChunkMs: firstChunkAt === null ? null : firstChunkAt - startedAt,
        receivedChars,
        payloadShortfall: receivedChars < fileBytes
      })
    })
  })
}

async function runLeg(label, runTrial) {
  const trials = []
  for (let i = 0; i < TRIAL_COUNT; i++) {
    const trial = await runTrial()
    trial.mbPerS = trialThroughput(trial)
    trials.push(trial)
    // Brief gap so one trial's teardown (child reaping, fd close) does not bleed
    // into the next trial's timing window.
    await new Promise((r) => setTimeout(r, 50))
  }
  return { label, mbPerS: median(trials.map((t) => t.mbPerS)), trials }
}

async function main() {
  const ptyModule = await import('node-pty')
  const { filePath, fileBytes, lineCount } = buildCorpusFile()
  const fileMB = fileBytes / BYTES_PER_MB
  console.log(`corpus: ${filePath} (${fileMB.toFixed(2)} MB, ${lineCount} lines)`)

  const ptyLeg = await runLeg('pty', () => runPtyTrial(ptyModule, filePath, fileBytes))
  const pipeLeg = await runLeg('pipe', () => runPipeTrial(filePath, fileBytes))

  const notes = [
    'MB/s counts chars actually delivered to the reader over wall-clock from spawn to last chunk (spawn latency included; firstChunkMs recorded per trial so it can be subtracted).',
    'pty leg delivers slightly more chars than the file on POSIX: the tty line discipline rewrites \\n to \\r\\n (ONLCR), ~0.8% inflation at 120-char lines.',
    'ratio = pty/pipe: how much of the plain-pipe rate survives the kernel tty layer.'
  ]
  if (process.platform === 'win32') {
    notes.push(
      'win32 leg uses `cmd /c type` and is UNTESTED — treat Windows figures as provisional until run on a real Windows host.'
    )
  }

  const report = {
    platform: `${process.platform} ${os.release()} ${os.arch()}`,
    nodeVersion: process.version,
    generatedAt: new Date().toISOString(),
    fileMB,
    lineWidth: LINE_WIDTH,
    trialCount: TRIAL_COUNT,
    childCommand: childCatCommand(filePath),
    legs: {
      pty: { mbPerS: ptyLeg.mbPerS, trials: ptyLeg.trials },
      pipe: { mbPerS: pipeLeg.mbPerS, trials: pipeLeg.trials }
    },
    ratio: ptyLeg.mbPerS / pipeLeg.mbPerS,
    notes
  }

  mkdirSync(resultsDir, { recursive: true })
  const stamp = new Date().toISOString().replace(/[:.]/g, '-')
  const outPath = path.join(resultsDir, `pty-floor-${stamp}.json`)
  writeFileSync(outPath, `${JSON.stringify(report, null, 2)}\n`)

  for (const leg of [ptyLeg, pipeLeg]) {
    const perTrial = leg.trials.map((t) => t.mbPerS.toFixed(1)).join(', ')
    console.log(
      `${leg.label.padEnd(5)} median ${leg.mbPerS.toFixed(1)} MB/s  (trials: ${perTrial})`
    )
  }
  console.log(`ratio (pty/pipe): ${report.ratio.toFixed(3)}`)
  const shortfalls = [...ptyLeg.trials, ...pipeLeg.trials].filter((t) => t.payloadShortfall)
  if (shortfalls.length > 0) {
    console.warn(
      `WARNING: ${shortfalls.length} trial(s) received fewer chars than the file payload — figures suspect.`
    )
  }
  console.log(`report: ${outPath}`)
}

main().catch((error) => {
  console.error(error)
  process.exitCode = 1
})
