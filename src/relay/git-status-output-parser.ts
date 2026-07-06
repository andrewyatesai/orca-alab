// The relay's `git status --porcelain=v2` parser now runs the orca-git Rust core
// via wasm (see git-wasm.ts) — the same code the main process runs via napi —
// instead of the hand-maintained TS reimplementation that used to live here and
// could drift from the Rust port. Re-exported to keep the import path stable.
export { parseStatusOutput } from './git-wasm'
