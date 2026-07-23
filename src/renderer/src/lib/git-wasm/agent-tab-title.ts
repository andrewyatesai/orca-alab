// Generated tab-title derivation, driven by the Rust orca-text core in the
// orca-git wasm module (the shared TS implementation was deleted; the Rust
// port carries the same 512-UTF-16-unit prompt-preview cap). The title is the
// cleaned first prompt clause — no post-generation identifier-first rewrite
// (removed in #9821 so naming defaults stay minimal and user overrides own it).
import { deriveGeneratedTabTitle as wasmDeriveGeneratedTabTitle } from './orca_git_wasm.js'
import { isGitWasmReady } from './git-line-stats'

/**
 * Short generated tab title from an agent prompt, or null when the prompt has
 * no usable title text — and null while the wasm initialises (the caller
 * keeps the tab's default title; a title generated a moment later is applied
 * by the next store update).
 */
export function deriveGeneratedTabTitle(prompt: string): string | null {
  if (!isGitWasmReady()) {
    return null
  }
  return wasmDeriveGeneratedTabTitle(prompt) ?? null
}
