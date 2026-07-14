// Renderer task-provider normalizers, driven by the Rust task-providers core in
// the orca-git wasm module (the shared TS impl was gutted to types + data).
// These run in sync render bodies and settings reducers, so every export
// returns a NON-NULL fallback when the wasm hasn't loaded yet — chosen to
// PRESERVE the user's saved providers rather than clobber the persisted list.
import { isGitWasmReady } from './git-line-stats'
import { orcaDispatch } from './orca_git_wasm.js'
import {
  TASK_PROVIDERS,
  type TaskProvider,
  type TaskProviderAvailability
} from '../../../../shared/task-providers'

function op(fn: string, input: unknown): unknown | null {
  if (!isGitWasmReady()) {return null}
  return JSON.parse(orcaDispatch('task-providers', fn, JSON.stringify(input ?? null)))
}

export function normalizeTaskProviderSettings(value: {
  visibleTaskProviders: unknown
  defaultTaskSource: unknown
}): { visibleTaskProviders: TaskProvider[]; defaultTaskSource: TaskProvider } {
  const r = op('normalizeTaskProviderSettings', value)
  // Pass the draft settings through untouched on wasm-load failure so the
  // settings reducer persists what it already had instead of a cleared record.
  return (r ?? value) as { visibleTaskProviders: TaskProvider[]; defaultTaskSource: TaskProvider }
}

export function normalizeVisibleTaskProviders(value: unknown): TaskProvider[] {
  const r = op('normalizeVisibleTaskProviders', value) as TaskProvider[] | null
  if (r) {return r}
  return Array.isArray(value) ? (value as TaskProvider[]) : [...TASK_PROVIDERS]
}

export function filterAvailableTaskProviders(
  visibleProviders: readonly TaskProvider[],
  availability: TaskProviderAvailability
): TaskProvider[] {
  const r = op('filterAvailableTaskProviders', { visibleProviders, availability }) as
    | TaskProvider[]
    | null
  if (r) {return r}
  return Array.isArray(visibleProviders) ? [...visibleProviders] : ['github']
}

export function restoreAvailableDefaultTaskProvider(
  visibleProviders: readonly TaskProvider[],
  availability: TaskProviderAvailability,
  preferredProvider: unknown
): TaskProvider[] {
  const r = op('restoreAvailableDefaultTaskProvider', {
    visibleProviders,
    availability,
    preferredProvider
  }) as TaskProvider[] | null
  return r ?? [...visibleProviders]
}

export function resolveVisibleTaskProvider(
  preferred: TaskProvider | null | undefined,
  visibleProviders: readonly TaskProvider[]
): TaskProvider {
  const r = op('resolveVisibleTaskProvider', { preferred, visibleProviders }) as TaskProvider | null
  return r ?? preferred ?? 'github'
}
