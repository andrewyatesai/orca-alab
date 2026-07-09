// Main-process task-query parser/serializer, driven by the Rust orca-core
// task_query port via napi (the shared TS impl was deleted). One source of truth
// with the parity-proven Rust port — the GitHub client parses the saved search
// string through the same core the renderer runs via wasm.
import { requireRustGitBinding } from './daemon/rust-git-addon'
import type { ParsedTaskQuery, TaskQueryFilterKey } from '../shared/task-query'

function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(
    requireRustGitBinding().orcaDispatch('task-query', fn, JSON.stringify(input ?? null))
  )
}

export function tokenizeSearchQuery(rawQuery: string): string[] {
  return dispatch('tokenizeSearchQuery', rawQuery) as string[]
}

export function parseTaskQuery(rawQuery: string): ParsedTaskQuery {
  return dispatch('parseTaskQuery', rawQuery) as ParsedTaskQuery
}

export function serializeTaskQuery(query: ParsedTaskQuery): string {
  return dispatch('serializeTaskQuery', query) as string
}

export function withQualifier(
  rawQuery: string,
  key: TaskQueryFilterKey,
  value: string | string[] | null
): string {
  return dispatch('withQualifier', { rawQuery, key, value }) as string
}

export function stripRepoQualifiers(rawQuery: string): string {
  return dispatch('stripRepoQualifiers', rawQuery) as string
}
