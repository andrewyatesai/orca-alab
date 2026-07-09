import type { OpenInApplication } from './types'

export const OPEN_IN_APPLICATIONS_MAX = 8
export const DEFAULT_OPEN_IN_APPLICATIONS: OpenInApplication[] = [
  { id: 'vscode', label: 'VS Code', command: 'code' }
]

// The normalizer moved to the Rust orca-config core (parity-proven); consumers
// now call the thin wrappers in src/main/rust-open-in-applications.ts (napi) and
// src/renderer/src/lib/git-wasm/open-in-applications.ts (wasm). This file keeps
// only the DEFAULT/MAX data + the option shape those wrappers share.
export type NormalizeOpenInApplicationsOptions = {
  createId?: () => string
  seedDefaults?: boolean
}
