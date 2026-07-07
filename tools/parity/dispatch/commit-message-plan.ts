// TS dispatch for the commit-message-plan parity module. The shared TS planner
// was DELETED (the Rust orca-agents core is the sole impl — napi in main, wasm
// in the renderer's dry-run preview), so this adapter drives the same wasm: the
// vectors' recorded goldens now pin that surface absolutely, and the harness's
// TS-vs-Rust diff degenerates to wasm-vs-binary.
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'planCommitMessageGeneration': {
      const { planInput, prompt } = input as { planInput: unknown; prompt: string }
      // Struct in/out crosses the wasm boundary as JSON.
      return JSON.parse(
        gitWasmOracle().planCommitMessageGeneration(JSON.stringify(planInput), prompt)
      )
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
