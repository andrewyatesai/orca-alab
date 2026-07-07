// Shared wasm entry for dispatch adapters whose TS oracle was DELETED (the
// Rust core is the sole implementation): the adapter drives the SAME wasm the
// relay/renderer run (initSync from the relay's embedded bytes — Node has no
// sync-compile restriction), so the vectors' recorded goldens keep pinning the
// production wasm surface, and the harness's TS-vs-Rust diff degenerates to
// wasm-vs-binary (drift between the two Rust entry points still surfaces).
import * as glue from '../../../src/relay/wasm/orca_git_wasm.js'
import { ORCA_GIT_WASM_BASE64 } from '../../../src/relay/wasm/orca_git_wasm_bg.wasm.base64'

let inited = false

export function gitWasmOracle(): typeof glue {
  if (!inited) {
    glue.initSync({ module: Buffer.from(ORCA_GIT_WASM_BASE64, 'base64') })
    inited = true
  }
  return glue
}
