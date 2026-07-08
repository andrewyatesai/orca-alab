// TS dispatch for the pull-request-generation parity module. The shared TS bodies
// were DELETED (the Rust orca-agents core is the sole impl — napi in main, wasm in
// the renderer's preview), so this adapter drives the SAME wasm: the vectors'
// recorded goldens pin that surface and the harness's TS-vs-Rust diff degenerates
// to wasm-vs-binary.
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'buildPullRequestFieldsPrompt': {
      const { context, customPrompt } = input as { context: unknown; customPrompt: string }
      // Struct in / string out crosses the wasm boundary as JSON.
      return gitWasmOracle().buildPullRequestFieldsPrompt(JSON.stringify(context), customPrompt)
    }
    case 'parseGeneratedPullRequestFields': {
      const { raw, fallback } = input as { raw: string; fallback: unknown }
      const result = JSON.parse(
        gitWasmOracle().parseGeneratedPullRequestFields(raw, JSON.stringify(fallback))
      ) as { ok: true; fields: unknown } | { ok: false; error: string }
      // Mirror the Rust dispatch: bare fields on success, parity-error marker else.
      return result.ok ? result.fields : { __parity_error__: result.error }
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
