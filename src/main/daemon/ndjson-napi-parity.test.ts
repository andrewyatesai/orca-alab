import { describe, expect, it } from 'vitest'
import { loadRustGitBinding } from './rust-git-addon'
import { createNdjsonParser } from './ndjson'

// Dual-run parity proof: the verified Rust `orca-net` NDJSON byte-budget splitter
// (exposed via the napi `NdjsonParser` class) must be a faithful drop-in for the
// live TS `createNdjsonParser`. Each fixture is fed to BOTH and asserted equal.
// This is the load-bearing evidence for a possible cut-over; it touches none of
// the live daemon-socket paths (the hot socket path stays TS — routing every
// chunk through the FFI would add copies V8 substrings avoid; a cut-over is
// bench-gated). Buffer length is UTF-8 BYTES on both sides, so multibyte fixtures
// exercise the exact byte-budget arithmetic.
//
// Skips cleanly when the .node is absent (CI without a native build).

const binding = loadRustGitBinding()
const suite = binding ? describe : describe.skip
// Safe under describe.skip: the closure never runs when the binding is null.
const git = binding!

type ParityCapture = { messages: unknown[]; oversized: number[]; parseErrors: number }

/** The TS `onError` receives BOTH oversized-line reports AND JSON.parse failures;
 *  split them so each maps to the napi side (oversized array vs a failed
 *  JSON.parse of a returned line). The byte count is extracted to prove the
 *  budget arithmetic matches, not just the event count. */
const OVERSIZED_RE = /NDJSON line exceeds max \d+ bytes \((\d+) bytes received\)/

function runTs(chunks: string[], maxLineBytes?: number): ParityCapture {
  const messages: unknown[] = []
  const oversized: number[] = []
  let parseErrors = 0
  const parser = createNdjsonParser(
    (msg) => messages.push(msg),
    (err) => {
      const match = OVERSIZED_RE.exec(err.message)
      if (match) {
        oversized.push(Number(match[1]))
      } else {
        parseErrors += 1
      }
    },
    maxLineBytes === undefined ? {} : { maxLineBytes }
  )
  for (const chunk of chunks) {
    parser.feed(chunk)
  }
  return { messages, oversized, parseErrors }
}

function runRust(chunks: string[], maxLineBytes?: number): ParityCapture {
  const messages: unknown[] = []
  const oversized: number[] = []
  let parseErrors = 0
  const parser = new git.NdjsonParser(maxLineBytes)
  for (const chunk of chunks) {
    const result = parser.feed(chunk)
    for (const bytes of result.oversized) {
      oversized.push(bytes)
    }
    for (const line of result.lines) {
      try {
        messages.push(JSON.parse(line))
      } catch {
        parseErrors += 1
      }
    }
  }
  return { messages, oversized, parseErrors }
}

function expectParity(chunks: string[], maxLineBytes?: number): void {
  expect(runRust(chunks, maxLineBytes)).toEqual(runTs(chunks, maxLineBytes))
}

suite('ndjson napi/TS parity', () => {
  it('splits complete lines in one chunk', () => {
    expectParity(['{"a":1}\n{"b":2}\n'])
  })

  it('reassembles messages split across chunk boundaries', () => {
    expectParity(['{"a":', '1}\n{"b"', ':2}\n{"c":', '3', '}\n'])
  })

  it('retains a partial (newline-less) tail across the last chunk', () => {
    expectParity(['{"done":1}\n{"partial":'])
  })

  it('skips empty lines without emitting a message or parse error', () => {
    expectParity(['\n\n{"x":1}\n\n{"y":2}\n'])
  })

  it('treats byte length as UTF-8 for multibyte (emoji surrogate-pair) content', () => {
    expectParity(['{"emoji":"😀🎉","tail":"z"}\n'])
  })

  it('treats byte length as UTF-8 for CJK content', () => {
    expectParity(['{"cjk":"日本語のテキスト"}\n'])
  })

  it('surfaces JSON.parse failures identically (invalid line, then valid)', () => {
    expectParity(['not json at all\n{"ok":1}\n'])
  })

  it('drops an oversized line and resyncs at the next newline (byte count matches)', () => {
    // maxLineBytes=8: the 13-byte garbage line trips the budget and is dropped;
    // the following 7-byte {"a":1} line parses cleanly.
    expectParity(['xxxxxxxxxxxxx\n{"a":1}\n'], 8)
  })

  it('discards an oversized line spanning multiple newline-less chunks', () => {
    // Long partial (no newline) → oversized + discarding; more garbage stays in
    // the discarding state; the newline resyncs and the valid line parses.
    expectParity(['xxxxxxxxxxxx', 'yyyyyyyyyyyy', 'zzz\n{"a":1}\n'], 8)
  })

  it('counts a multibyte oversized line by bytes, not chars', () => {
    // 3 emoji = 12 UTF-8 bytes > 8 budget; the reported observed byte count must
    // match on both sides (proves char-vs-byte parity in the budget check).
    expectParity(['😀😀😀\n{"a":1}\n'], 8)
  })

  it('resets partial + discarding state identically', () => {
    const tsMessages: unknown[] = []
    const tsParser = createNdjsonParser((msg) => tsMessages.push(msg))
    tsParser.feed('{"partial":')
    tsParser.reset()
    tsParser.feed('{"after":1}\n')

    const rustMessages: unknown[] = []
    const rustParser = new git.NdjsonParser()
    rustParser.feed('{"partial":')
    rustParser.reset()
    for (const line of rustParser.feed('{"after":1}\n').lines) {
      rustMessages.push(JSON.parse(line))
    }

    expect(rustMessages).toEqual(tsMessages)
    expect(rustMessages).toEqual([{ after: 1 }])
  })
})
