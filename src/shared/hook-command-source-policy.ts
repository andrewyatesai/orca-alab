// Logic moved to the Rust hook-command-source-policy core (orca-core): main
// drives resolveHookCommandSourcePolicy via napi
// (src/main/rust-hook-command-source-policy.ts), the renderer via wasm
// (src/renderer/src/lib/git-wasm/hook-command-source-policy.ts). This file keeps
// only the policy TYPE so it stays import-safe from every surface (no napi/wasm
// import here).
export type { HookCommandSourcePolicy } from './types'
