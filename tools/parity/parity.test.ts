// Differential parity driver (TS half).
//
// Proves the fresh Rust ports against the live TypeScript reference: for every
// case in the shared vector corpus it runs the real `src/shared` function and
// asserts its output equals the Rust port's output (read from rust_outputs.json,
// produced by `cargo run -p orca-parity`). The compared outputs are both
// computed live — neither is hand-authored — so an agreement is real evidence
// of behavioural parity, and a disagreement is a concrete divergence to review.
//
// Run order:
//   1. cd rust && cargo run -p orca-parity -- ../tools/parity/vectors ../tools/parity/rust_outputs.json
//   2. vitest run tools/parity/parity.test.ts

import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { semanticEqual } from './compare'
import { DISPATCH } from './dispatch'

const HERE = __dirname
const VECTORS_DIR = join(HERE, 'vectors')
const RUST_OUTPUTS = join(HERE, 'rust_outputs.json')

type VectorCase = {
  function: string
  note?: string
  input: unknown
  expected?: unknown
  /** Mark an intended fresh-reimplementation divergence; reported, never failed. */
  allowDivergence?: string
}

type RustRun = {
  module: string
  caseIndex: number
  function: string
  rustOutput: unknown
}

const rustRuns: RustRun[] = existsSync(RUST_OUTPUTS)
  ? (JSON.parse(readFileSync(RUST_OUTPUTS, 'utf8')) as RustRun[])
  : []
const rustByKey = new Map(rustRuns.map((run) => [`${run.module}::${run.caseIndex}`, run]))

describe('TS↔Rust parity', () => {
  it('rust_outputs.json exists (run `cargo run -p orca-parity` first)', () => {
    expect(existsSync(RUST_OUTPUTS)).toBe(true)
  })

  const vectorFiles = existsSync(VECTORS_DIR)
    ? readdirSync(VECTORS_DIR).filter((name) => name.endsWith('.json'))
    : []

  for (const file of vectorFiles) {
    const doc = JSON.parse(readFileSync(join(VECTORS_DIR, file), 'utf8')) as {
      module: string
      cases: VectorCase[]
    }
    const dispatcher = DISPATCH[doc.module]

    describe(doc.module, () => {
      it('has a TS dispatch adapter', () => {
        expect(dispatcher, `no TS dispatch registered for module "${doc.module}"`).toBeTypeOf('function')
      })

      doc.cases.forEach((vectorCase, index) => {
        const label = `${vectorCase.function} #${index}${vectorCase.note ? ` — ${vectorCase.note}` : ''}`
        it(label, () => {
          if (!dispatcher) return
          const tsOutput = dispatcher(vectorCase.function, vectorCase.input)
          const rustRun = rustByKey.get(`${doc.module}::${index}`)
          expect(rustRun, `no Rust output for ${doc.module}#${index} — re-run the Rust harness`).toBeTruthy()

          const matches = semanticEqual(tsOutput, rustRun!.rustOutput)
          const detail =
            `\n  input:    ${JSON.stringify(vectorCase.input)}` +
            `\n  ts:       ${JSON.stringify(tsOutput)}` +
            `\n  rust:     ${JSON.stringify(rustRun!.rustOutput)}`

          if (vectorCase.allowDivergence) {
            // Intended fresh-reimplementation difference: report, do not fail.
            if (!matches) console.warn(`KNOWN DIVERGENCE (${vectorCase.allowDivergence})${detail}`)
            return
          }

          expect(matches, `DIVERGENCE: TS and Rust disagree${detail}`).toBe(true)

          if (vectorCase.expected !== undefined) {
            expect(
              semanticEqual(tsOutput, vectorCase.expected),
              `TS output disagrees with the golden expected value${detail}`
            ).toBe(true)
          }
        })
      })
    })
  }
})
