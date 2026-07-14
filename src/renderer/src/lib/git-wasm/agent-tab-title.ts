// Generated tab-title derivation, driven by the Rust orca-text core in the
// orca-git wasm module (the shared TS implementation was deleted; the Rust
// port carries the same 512-UTF-16-unit prompt-preview cap). The review-target
// identifier lead is applied here on top of the Rust cleaned title, reusing the
// shared work-item-reference parser (also used by the sidebar workspace name).
import { deriveGeneratedTabTitle as wasmDeriveGeneratedTabTitle } from './orca_git_wasm.js'
import { isGitWasmReady } from './git-line-stats'
import {
  GENERATED_TAB_TITLE_MAX_LENGTH,
  GENERATED_TAB_TITLE_SOURCE_SCAN_LIMIT
} from '../../../../shared/agent-tab-title'
import {
  extractWorkIdentifier,
  formatIdentifierFirst,
  stripWorkIdentifierEcho
} from '../../../../shared/work-item-reference'

// Faithful ports of the Rust orca-text helpers (agent_tab_title.rs), needed here
// because the identifier lead is applied in TS on the cleaned Rust title.
function capitalizeFirstLetter(value: string): string {
  return value.replace(/\p{L}/u, (letter) => letter.toUpperCase())
}

function truncateAtWordBoundary(value: string, maxLength: number): string {
  const chars = [...value]
  if (chars.length <= maxLength) {
    return value
  }
  const rawSlice = chars.slice(0, maxLength).join('')
  const sliced = rawSlice.trim()
  if ([...sliced].length < chars.slice(0, maxLength).length) {
    return sliced
  }
  const slicedChars = [...sliced]
  const threshold = Math.floor(maxLength * 0.55)
  const lastSpace = slicedChars.lastIndexOf(' ')
  if (lastSpace >= threshold) {
    return slicedChars.slice(0, lastSpace).join('').trim()
  }
  return sliced
}

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
  const candidate = wasmDeriveGeneratedTabTitle(prompt) ?? null
  // Lead with the review target (`PR 1094 - …`) so the tab matches the sidebar
  // workspace name; the number is recovered from the raw prompt because the
  // Rust cleaning pipeline strips the URL/prefix from the cleaned title.
  const identifier = extractWorkIdentifier(prompt.slice(0, GENERATED_TAB_TITLE_SOURCE_SCAN_LIMIT))
  if (identifier && candidate) {
    const detail = capitalizeFirstLetter(stripWorkIdentifierEcho(candidate, identifier))
    return truncateAtWordBoundary(
      formatIdentifierFirst(identifier.label, detail),
      GENERATED_TAB_TITLE_MAX_LENGTH
    )
  }
  return candidate
}
