// TS dispatch for the terminal-quick-commands parity module. The shared TS bodies
// were DELETED (the Rust orca-agents core is the sole impl — napi in main, wasm in
// the renderer), so this adapter drives the SAME wasm through the single
// `terminalQuickCommandOp` boundary: the vectors' goldens pin that surface and the
// harness's TS-vs-Rust diff degenerates to wasm-vs-binary.
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(gitWasmOracle().terminalQuickCommandOp(fn, JSON.stringify(input ?? null)))
}
