// Impl DELETED — the Rust orca-core `base_ref_search_result` port is the sole
// implementation (the renderer drives it via the orca-git wasm; parity pins
// that surface). Only the result type remains, re-exported so consumers keyed
// off this module path keep resolving without a napi/wasm import in src/shared.
export type { BaseRefSearchResult } from './types'
