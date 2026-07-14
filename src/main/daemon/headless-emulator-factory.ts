import { appendFileSync } from 'node:fs'
import {
  HeadlessEmulator,
  type HeadlessEmulatorOptions,
  type HeadlessEmulatorWriteOptions
} from './headless-emulator'
import type { TerminalSnapshot } from './types'
import type { TerminalViewAttributes } from '../../shared/terminal-view-attributes'

// The emulator surface the daemon session depends on. The aterm-backed
// HeadlessEmulator satisfies it (and a richer API for the runtime/history paths).
export type TerminalEmulator = {
  write(data: string, opts?: HeadlessEmulatorWriteOptions): Promise<void> | void
  /** Synchronous seed/replay write for cold-restore; never forwards replies. */
  writeSync(data: string): boolean
  resize(cols: number, rows: number): void
  getAppliedSize(): { cols: number; rows: number }
  getSnapshot(opts?: { scrollbackRows?: number }): TerminalSnapshot
  /** Dangling incomplete escape at the stream position (empty when none). */
  readonly partialEscapeTailAnsi: string
  getCwd(): string | null
  clearScrollback(): void
  // Gate for the Ctrl+K PTY-buffer clear: only form-feed when the cursor sits on
  // an empty prompt, so PSReadLine/ConPTY doesn't repaint at a stale row.
  isCursorOnEmptyPromptLine(): boolean
  // Query-authority surface (terminal-query-authority.md). The daemon Session
  // never calls these (its emulator carries no reply sink); the runtime
  // per-PTY emulators drive them via the concrete HeadlessEmulator.
  installViewAttributeResponder(getter: () => TerminalViewAttributes | null): void
  applyPushedViewAttributes(attributes: TerminalViewAttributes): void
  installConptyPrimaryDeviceAttributesOverride(): void
  applyKittyKeyboardFlags(flags: number): Promise<void>
  disableQueryReplyForwarding(): void
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
