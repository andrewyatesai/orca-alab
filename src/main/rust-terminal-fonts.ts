// Main-process terminal font-weight resolver, driven by the Rust
// `orca_core::terminal_fonts` port via napi (the shared TS impl was deleted).
// One source of truth with the parity-proven Rust port. The vector input is a
// bare number, so we stringify the weight directly (matching the Rust dispatch's
// `input.as_f64()`).
import { requireRustGitBinding } from './daemon/rust-git-addon'

export function resolveTerminalFontWeights(fontWeight: number | null | undefined): {
  fontWeight: number
  fontWeightBold: number
} {
  return JSON.parse(
    requireRustGitBinding().orcaDispatch(
      'terminal-fonts',
      'resolveTerminalFontWeights',
      JSON.stringify(fontWeight ?? null)
    )
  ) as { fontWeight: number; fontWeightBold: number }
}
