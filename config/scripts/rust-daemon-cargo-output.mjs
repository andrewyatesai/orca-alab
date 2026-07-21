import { StringDecoder } from 'node:string_decoder'
import { Transform } from 'node:stream'

const TEMPORAL_PROOF_RECEIPT =
  'ScrollbackOffloadWindow proven (Buggy=0) and non-vacuous (Buggy=1) by Trust ty'
const TEMPORAL_PROOF_SUCCESS_RE = new RegExp(
  `^warning: aterm-grid@[^:\\r\\n]+: temporal gate ✓ (${TEMPORAL_PROOF_RECEIPT.replace(
    /[.*+?^${}()|[\]\\]/g,
    '\\$&'
  )})$`
)
const ANSI_SGR_RE = new RegExp(`${String.fromCharCode(27)}\\[[0-9;]*m`, 'g')

function classifySegment(segment) {
  const newline = segment.endsWith('\n') ? '\n' : ''
  const content = newline ? segment.slice(0, -1) : segment
  const carriageReturn = content.endsWith('\r') ? '\r' : ''
  const line = carriageReturn ? content.slice(0, -1) : content
  // `CARGO_TERM_COLOR=always` wraps diagnostics in SGR styling even though
  // stderr is piped. Match against plain text, but retain non-proof lines with
  // their original bytes and color sequences intact.
  const match = TEMPORAL_PROOF_SUCCESS_RE.exec(line.replace(ANSI_SGR_RE, ''))

  return match
    ? { receipt: match[1], retained: '', lineEnding: `${carriageReturn}${newline}` }
    : { receipt: null, retained: segment, lineEnding: '' }
}

/**
 * Cargo only gives build scripts a warning channel for visible status output.
 * Reclassify aterm's successful proof receipt while preserving every real
 * compiler and build-script warning byte-for-byte on stderr.
 */
export function classifyRustDaemonCargoStderr(stderr) {
  const proofReceipts = []
  const retained = []

  for (const segment of stderr.match(/.*(?:\n|$)/g) ?? []) {
    if (!segment) {
      continue
    }
    const result = classifySegment(segment)
    if (result.receipt) {
      proofReceipts.push(result.receipt)
    } else {
      retained.push(result.retained)
    }
  }

  return { stderr: retained.join(''), proofReceipts }
}

/**
 * Stream Cargo stderr while replacing only the successful temporal-proof
 * warning. Cargo output can be large and long-running, so callers must not
 * buffer the whole build merely to reclassify this one build-script line.
 */
export function createCargoTemporalProofStderrFilter(label) {
  const decoder = new StringDecoder('utf8')
  let pending = ''
  const verifiedPrefix = `[${label}] verified `

  function emitCompleteLines(stream) {
    let newlineIndex = pending.indexOf('\n')
    while (newlineIndex !== -1) {
      const segment = pending.slice(0, newlineIndex + 1)
      pending = pending.slice(newlineIndex + 1)
      const result = classifySegment(segment)
      stream.push(
        result.receipt ? `${verifiedPrefix}${result.receipt}${result.lineEnding}` : result.retained
      )
      newlineIndex = pending.indexOf('\n')
    }
  }

  return new Transform({
    transform(chunk, _encoding, callback) {
      pending += decoder.write(chunk)
      emitCompleteLines(this)
      callback()
    },
    flush(callback) {
      pending += decoder.end()
      if (pending) {
        const result = classifySegment(pending)
        this.push(
          result.receipt
            ? `${verifiedPrefix}${result.receipt}${result.lineEnding}`
            : result.retained
        )
      }
      callback()
    }
  })
}

export function createRustDaemonCargoStderrFilter() {
  return createCargoTemporalProofStderrFilter('build-rust-daemon')
}
