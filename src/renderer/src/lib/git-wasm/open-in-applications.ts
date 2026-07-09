// Renderer open-in-applications sanitizer, driven by the Rust orca-config core in
// the orca-git wasm module (the shared TS body was deleted; the DEFAULT/MAX data
// + type stay in TS). The only call site is the settings reducer at
// user-interaction time (well after the eager startGitWasm), and the main
// process re-normalizes authoritatively on set — so the pre-ready fallback
// (input array returned unchanged) is defensive only and never persists
// un-normalized data.
import { orcaDispatch } from './orca_git_wasm.js'
import { isGitWasmReady } from './git-line-stats'
import {
  DEFAULT_OPEN_IN_APPLICATIONS,
  type NormalizeOpenInApplicationsOptions
} from '../../../../shared/open-in-applications'
import type { OpenInApplication } from '../../../../shared/types'

export function normalizeOpenInApplications(
  value: unknown,
  options: NormalizeOpenInApplicationsOptions = {}
): OpenInApplication[] {
  if (!isGitWasmReady()) {
    // Never null into the sync settings reducer: keep the caller's array (main
    // re-normalizes authoritatively on set) or seed defaults when it's missing.
    if (Array.isArray(value)) {
      return value as OpenInApplication[]
    }
    return options.seedDefaults ? [...DEFAULT_OPEN_IN_APPLICATIONS] : []
  }
  // Reify the optional id generator as a plain-JSON pool the Rust closure pops
  // in sequence (one fresh id per row is an upper bound on blank-id rows).
  const createIds =
    Array.isArray(value) && options.createId ? value.map(() => options.createId!()) : undefined
  return JSON.parse(
    orcaDispatch(
      'open-in-applications',
      'normalizeOpenInApplications',
      JSON.stringify({ value, seedDefaults: options.seedDefaults ?? false, createIds })
    )
  ) as OpenInApplication[]
}
