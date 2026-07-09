// TS dispatch for the terminal-fonts parity module. The shared TS bodies were
// DELETED (the Rust orca_core::terminal_fonts port is the sole impl — the
// renderer drives it via the orca-git wasm), so this adapter drives the same
// wasm: the vectors' recorded goldens now pin that surface, and the harness's
// TS-vs-Rust diff degenerates to wasm-vs-binary (drift between the two Rust
// entry points would still surface here).
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  // Both functions take a single bare-number arg; JSON null carries TS
  // null/undefined (Rust maps None -> the Orca default weight).
  const json = gitWasmOracle().orcaDispatch('terminal-fonts', fn, JSON.stringify(input ?? null))
  return JSON.parse(json)
}
