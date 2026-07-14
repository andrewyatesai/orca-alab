// Global test setup: bind the shared Rust dispatch seam to the embedded wasm
// core, so src/shared modules cut over to Rust (via tryOrcaDispatch /
// requireOrcaDispatch) work in unit tests without each surface's production
// bootstrap. Uses the base64-embedded wasm (committed, always available) — the
// byte-identical twin of the napi core. Renderer tests that init their own wasm
// (initGitWasmForTestFromBytes → markReady) simply rebind to the same core.
import { initSync, orcaDispatch } from '../src/relay/wasm/orca_git_wasm.js'
import { ORCA_GIT_WASM_BASE64 } from '../src/relay/wasm/orca_git_wasm_bg.wasm.base64'
import { setOrcaDispatchBinding } from '../src/shared/orca-dispatch-seam'

initSync({ module: Buffer.from(ORCA_GIT_WASM_BASE64, 'base64') })
setOrcaDispatchBinding((module, fn, inputJson) => orcaDispatch(module, fn, inputJson))
