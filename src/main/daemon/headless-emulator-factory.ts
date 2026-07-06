import { appendFileSync } from 'node:fs'
import { HeadlessEmulator, type HeadlessEmulatorOptions } from './headless-emulator'
import type { TerminalSnapshot } from './types'

// The emulator surface the daemon session depends on. The aterm-backed
// HeadlessEmulator satisfies it (and a richer API for the runtime/history paths).
export type TerminalEmulator = {
  write(data: string): void
  resize(cols: number, rows: number): void
  getAppliedSize(): { cols: number; rows: number }
  getSnapshot(): TerminalSnapshot
  getCwd(): string | null
  clearScrollback(): void
  // Gate for the Ctrl+K PTY-buffer clear: only form-feed when the cursor sits on
  // an empty prompt, so PSReadLine/ConPTY doesn't repaint at a stale row.
  isCursorOnEmptyPromptLine(): boolean
  dispose(): void
}

let loggedSelection = false

function markEngine(engine: string): void {
  // Diagnostic: the daemon runs with stdio 'ignore', so console.log is invisible.
  // When ORCA_ENGINE_MARKER is set, record the engine to that file so an E2E
  // harness can prove which emulator went live. Gated on the env var.
  const marker = process.env.ORCA_ENGINE_MARKER
  if (!marker) {
    return
  }
  try {
    appendFileSync(marker, `${engine}\n`)
  } catch {
    // diagnostic only — never break session creation
  }
}

/** Build the daemon terminal emulator. Always the aterm (Rust) engine; the
 *  constructor throws if the native addon is missing, since there is no longer a
 *  TypeScript/xterm fallback. */
export function createHeadlessEmulator(opts: HeadlessEmulatorOptions): TerminalEmulator {
  const emulator = new HeadlessEmulator(opts)
  if (!loggedSelection) {
    loggedSelection = true
    // One-time proof, in the daemon log, that the native engine is live.
    console.log('[orca] terminal engine: aterm (Rust)')
  }
  markEngine('rust:aterm')
  return emulator
}
