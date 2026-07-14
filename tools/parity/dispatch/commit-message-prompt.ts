// TS dispatch for the commit-message-prompt parity module. cleanGeneratedCommitMessage
// was cut over to the Rust core (main via napi through the dispatch seam), so this
// adapter drives the SAME wasm for it — the diff degenerates to wasm-vs-binary and
// the goldens pin correctness. buildCommitPrompt stays live TS (dead in production
// but retained as the parity reference).
import { buildCommitPrompt } from '../../../src/shared/commit-message-prompt'
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'buildCommitPrompt': {
      const { diff, suffix } = input as { diff: string; suffix: string }
      return buildCommitPrompt(diff, suffix)
    }
    case 'cleanGeneratedCommitMessage':
      return JSON.parse(
        gitWasmOracle().orcaDispatch('commit-message-prompt', fn, JSON.stringify(input ?? null))
      )
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
