import { spawn, spawnSync } from 'node:child_process'
import { constants as osConstants } from 'node:os'
import { finished } from 'node:stream/promises'
import { createCargoTemporalProofStderrFilter } from './rust-daemon-cargo-output.mjs'

export class CargoCommandFailure extends Error {
  constructor(message, exitCode = 1, reason = 'exit') {
    super(message)
    this.exitCode = exitCode
    this.reason = reason
  }
}

function signalCargoTree(child, signal) {
  if (!child.pid || child.exitCode !== null || child.signalCode !== null) {
    return
  }
  if (process.platform === 'win32') {
    // Node's child.kill() only terminates the direct process on Windows.
    // taskkill /T covers rustc and build-script descendants as well. Windows
    // has no equivalent of POSIX stop/continue signals, so this helper is only
    // called for termination there.
    const result = spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], {
      stdio: 'ignore',
      windowsHide: true
    })
    if (result.status !== 0) {
      child.kill(signal)
    }
    return
  }

  try {
    // Cargo is spawned as a process-group leader below, so a negative PID
    // forwards cancellation to Cargo, rustc, and build-script descendants.
    process.kill(-child.pid, signal)
  } catch (error) {
    if (error?.code !== 'ESRCH') {
      child.kill(signal)
    }
  }
}

/**
 * Run Cargo without buffering its output, replacing only aterm's exact
 * successful temporal-proof diagnostic with a labelled verified receipt.
 */
export async function runStreamedCargoCommand({ command, args, cwd, env, label, shell = false }) {
  const child = spawn(command, args, {
    detached: process.platform !== 'win32',
    stdio: ['inherit', 'inherit', 'pipe'],
    cwd,
    env,
    shell
  })

  const stderrFilter = createCargoTemporalProofStderrFilter(label)
  child.stderr.pipe(stderrFilter).pipe(process.stderr, { end: false })

  // A signal sent specifically to this Node wrapper (rather than its whole
  // terminal process group) must not orphan Cargo or leave target locks held.
  const forwardedSignals =
    process.platform === 'win32'
      ? ['SIGINT', 'SIGTERM']
      : ['SIGHUP', 'SIGINT', 'SIGQUIT', 'SIGTERM']
  let forwardedSignal = null
  const signalHandlers = new Map(
    forwardedSignals.map((signal) => [
      signal,
      () => {
        forwardedSignal ??= signal
        signalCargoTree(child, signal)
      }
    ])
  )
  for (const [signal, handler] of signalHandlers) {
    process.on(signal, handler)
  }

  // `detached` gives Cargo its own POSIX process group so cancellation can
  // reach the complete compiler tree. Mirror terminal job control explicitly:
  // stop Cargo's group before stopping this wrapper, then continue it when the
  // shell foregrounds the wrapper again.
  const handleStop = () => {
    // Detached POSIX groups are orphaned, and POSIX permits SIGTSTP to be
    // ignored for orphaned groups. SIGSTOP guarantees the compiler tree stops.
    signalCargoTree(child, 'SIGSTOP')
    process.kill(process.pid, 'SIGSTOP')
  }
  const handleContinue = () => signalCargoTree(child, 'SIGCONT')
  if (process.platform !== 'win32') {
    process.on('SIGTSTP', handleStop)
    process.on('SIGCONT', handleContinue)
  }

  const exitResult = new Promise((resolveExit, rejectExit) => {
    child.once('error', (error) => {
      rejectExit(new CargoCommandFailure(`could not start cargo: ${error.message}`, 1, 'spawn'))
    })
    child.once('close', (status, signal) => resolveExit({ status, signal }))
  })
  let status
  let childSignal
  try {
    const [result] = await Promise.all([exitResult, finished(stderrFilter)])
    status = result.status
    childSignal = result.signal
  } finally {
    for (const [signal, handler] of signalHandlers) {
      process.off(signal, handler)
    }
    if (process.platform !== 'win32') {
      process.off('SIGTSTP', handleStop)
      process.off('SIGCONT', handleContinue)
    }
  }

  const terminatingSignal = childSignal ?? forwardedSignal
  if (terminatingSignal) {
    const signalNumber = osConstants.signals[terminatingSignal]
    throw new CargoCommandFailure(
      `cargo build terminated by ${terminatingSignal}`,
      signalNumber ? 128 + signalNumber : 1,
      'signal'
    )
  }
  if (status !== 0) {
    throw new CargoCommandFailure(`cargo build failed (exit ${status})`, status ?? 1)
  }
}
