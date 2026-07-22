// Wire-level pieces of the daemon flood harness (daemon-flood-timed.mjs): the
// hello/createOrAttach lines the client sends, the raw-byte exit-event scanner
// that receiver-times the flood, and the ssh argv used by ssh-localhost mode.
// Kept IO-free so every byte-level decision is unit-testable.

// Must track PROTOCOL_VERSION in rust/crates/orca-daemon/src/protocol.rs —
// 1020 is the binary-stream rev the harness class is named after.
export const DAEMON_PROTOCOL_VERSION = 1020

// First line on each daemon socket (src/main/daemon/types.ts hello shape).
// Token is empty: the harness launches the daemon token-less (parity mode).
export function helloLine(role, { binaryStream = false, clientId = 'flood' } = {}) {
  const hello = { type: 'hello', version: DAEMON_PROTOCOL_VERSION, token: '', clientId, role }
  if (role === 'stream' && binaryStream) {
    hello.streamFormat = 'binary'
  }
  return `${JSON.stringify(hello)}\n`
}

// The flood session: the child argv IS the flood (no login-shell rc noise).
export function createOrAttachLine({
  id,
  sessionId,
  corpusPath,
  platform = process.platform,
  cols = 120,
  rows = 40
}) {
  // Why: `sh -c 'cat "$1"' flood <path>` keeps corpus paths with spaces intact.
  const spawnSpec =
    platform === 'win32'
      ? { shellOverride: 'cmd.exe', shellArgs: ['/d', '/s', '/c', `type "${corpusPath}"`] }
      : { shellOverride: '/bin/sh', shellArgs: ['-c', 'cat "$1"', 'flood', corpusPath] }
  return `${JSON.stringify({
    id,
    type: 'createOrAttach',
    payload: { sessionId, cols, rows, ...spawnSpec }
  })}\n`
}

// The session-exit marker works for BOTH stream formats: NDJSON carries the
// exit event as a JSON line, and a v1020 binary Event frame carries the exact
// same JSON text as its payload (protocol.rs::event_frame). Corpus data cannot
// collide with it — flood lines are plain SGR-colored ASCII prose.
const EXIT_NEEDLE = Buffer.from('"event":"exit"')

// Receiver-timed drain scanner: feed raw socket chunks, get `true` once the
// exit event has been consumed. Counts wire bytes up to and including the
// chunk that carried the marker (matching stream_flood_bench.rs).
export function makeExitEventScanner() {
  let tail = Buffer.alloc(0)
  let wireBytes = 0
  let sawExit = false
  return {
    get wireBytes() {
      return wireBytes
    },
    get sawExit() {
      return sawExit
    },
    push(chunk) {
      if (sawExit) {
        return true
      }
      wireBytes += chunk.length
      // Why: a tail overlap of needle-1 bytes catches a marker that straddles a read boundary.
      const scan = tail.length > 0 ? Buffer.concat([tail, chunk]) : chunk
      if (scan.includes(EXIT_NEEDLE)) {
        sawExit = true
        return true
      }
      const keep = Math.min(scan.length, EXIT_NEEDLE.length - 1)
      tail = Buffer.from(scan.subarray(scan.length - keep))
      return false
    }
  }
}

// ssh argv for the tunnel that carries the stream across a real SSH transport:
// `-L <local.sock>:<remote.sock>` Unix-socket forwarding (OpenSSH ≥ 6.7).
export function sshForwardArgs({ destination, localSocket, remoteSocket, extraSshArgs = [] }) {
  return [
    '-N',
    '-o',
    'BatchMode=yes',
    '-o',
    'ExitOnForwardFailure=yes',
    // Why: ssh refuses to bind an existing socket file; unlink-first makes reruns idempotent.
    '-o',
    'StreamLocalBindUnlink=yes',
    '-o',
    'StrictHostKeyChecking=accept-new',
    ...extraSshArgs,
    '-L',
    `${localSocket}:${remoteSocket}`,
    destination
  ]
}

// Cheap auth/reachability check before spending a corpus generation + daemon
// launch on a tunnel that cannot come up.
export function sshPreflightArgs({ destination, extraSshArgs = [] }) {
  return [
    '-o',
    'BatchMode=yes',
    '-o',
    'StrictHostKeyChecking=accept-new',
    ...extraSshArgs,
    destination,
    'true'
  ]
}

// Trial summary stats — median first (the investigation doc reports medians).
export function summarizeRates(values) {
  if (values.length === 0) {
    throw new Error('summarizeRates: no values')
  }
  const sorted = [...values].sort((a, b) => a - b)
  const mid = Math.floor(sorted.length / 2)
  const median = sorted.length % 2 === 1 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2
  const mean = sorted.reduce((a, b) => a + b, 0) / sorted.length
  return { median, mean, min: sorted[0], max: sorted.at(-1) }
}
