// Loader for the conformance corpus. Payloads are base64 ("bytes_b64") because
// JSON strings decoded as latin1 silently truncate codepoints >0xFF — that bug
// meant the UTF-8 cases never actually tested UTF-8, and deliberately-invalid
// byte sequences were inexpressible.
import { readFileSync } from 'node:fs'

export function loadCorpus(path) {
  return JSON.parse(readFileSync(path, 'utf8')).map((c) => {
    if (typeof c.bytes_b64 !== 'string') {
      throw new Error(`corpus case "${c.name}" has no bytes_b64 — encode raw bytes as base64`)
    }
    return { ...c, bytes: Buffer.from(c.bytes_b64, 'base64') }
  })
}

// Loader for the spec-cited differential corpus (tools/conformance/cases.jsonl):
// one JSON object per line with hex payloads and per-case grid dimensions. These
// carry an xterm oracle (goldens.jsonl, regenerated from @xterm/headless) so they
// slot straight into the aterm-vs-xterm differential. `feature` becomes the
// divergence comment; `cols`/`rows` size the grid (many cases are intentionally
// small, e.g. 20x6, to exercise wrap/clamp at edges).
export function loadJsonlCorpus(path) {
  return readFileSync(path, 'utf8')
    .split('\n')
    .filter((line) => line.trim() !== '')
    .map((line) => {
      const c = JSON.parse(line)
      if (typeof c.bytesHex !== 'string') {
        throw new Error(`jsonl corpus case "${c.id}" has no bytesHex — encode raw bytes as hex`)
      }
      return {
        name: c.id,
        bytes: Buffer.from(c.bytesHex, 'hex'),
        cols: c.cols,
        rows: c.rows,
        comment: c.feature
      }
    })
}
