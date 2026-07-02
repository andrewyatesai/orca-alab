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
