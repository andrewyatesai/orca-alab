// Main-process task-provider normalizers, driven by the Rust task-providers
// core via napi (the shared TS impl was gutted to types + data). One source of
// truth with the parity-proven Rust port; main only consumes the guard and the
// settings normalizer, so those are the sole exports here.
import { requireRustGitBinding } from './daemon/rust-git-addon'
import type { TaskProvider } from '../shared/task-providers'

function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(
    requireRustGitBinding().orcaDispatch('task-providers', fn, JSON.stringify(input ?? null))
  )
}

export function isTaskProvider(value: unknown): value is TaskProvider {
  return dispatch('isTaskProvider', value) as boolean
}

export function normalizeTaskProviderSettings(value: {
  visibleTaskProviders: unknown
  defaultTaskSource: unknown
}): { visibleTaskProviders: TaskProvider[]; defaultTaskSource: TaskProvider } {
  return dispatch('normalizeTaskProviderSettings', value) as {
    visibleTaskProviders: TaskProvider[]
    defaultTaskSource: TaskProvider
  }
}
