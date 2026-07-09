// Task-query TYPES only. The behavior (tokenize/parse/serialize/withQualifier/
// stripRepoQualifiers) moved to the parity-proven Rust orca-core task_query
// port: main drives it via napi (src/main/rust-task-query.ts), the renderer via
// wasm (src/renderer/src/lib/git-wasm/task-query.ts). Kept import-safe from every
// surface (no napi/wasm import here).

export type ParsedTaskQuery = {
  scope: 'all' | 'issue' | 'pr'
  state: 'open' | 'closed' | 'all' | 'merged' | null
  draft: boolean
  assignee: string | null
  author: string | null
  reviewRequested: string | null
  reviewedBy: string | null
  labels: string[]
  freeText: string
}

export type TaskQueryFilterKey =
  | 'author'
  | 'assignee'
  | 'reviewRequested'
  | 'reviewedBy'
  | 'labels'
  | 'state'
  | 'draft'
