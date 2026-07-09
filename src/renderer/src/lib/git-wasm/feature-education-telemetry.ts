// Feature-education telemetry source normalizers, driven by the Rust
// orca-config core via the orca-git wasm module (the shared TS bodies were
// gutted; the enum DATA consts + types stay in src/shared). Both consumers are
// null-safe track() emitters, so an off-table value or the ~tens-of-ms wasm
// boot window degrades to the same bounded 'unknown' fallback the Rust port
// returns.
import { isGitWasmReady } from './git-line-stats'
import { orcaDispatch } from './orca_git_wasm.js'
import type {
  FeatureEducationSource,
  SetupGuideSource
} from '../../../../shared/feature-education-telemetry'

function op(fn: string, input: unknown): unknown | null {
  if (!isGitWasmReady()) {
    return null
  }
  return JSON.parse(orcaDispatch('feature-education-telemetry', fn, JSON.stringify(input ?? null)))
}

export function normalizeFeatureEducationSource(
  value: string | null | undefined
): FeatureEducationSource {
  const r = op('normalizeFeatureEducationSource', value) as FeatureEducationSource | null
  return r ?? 'unknown'
}

export function normalizeSetupGuideSource(value: string | null | undefined): SetupGuideSource {
  const r = op('normalizeSetupGuideSource', value) as SetupGuideSource | null
  return r ?? 'unknown'
}
