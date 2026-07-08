// Types for the pull-request field generator. The prompt-build + reply-parse
// bodies were DELETED — the Rust `orca-agents::pull_request_generation` core is
// the sole impl (napi in main via ./text-generation/rust-pull-request-generation,
// wasm in the renderer's dry-run preview). See that crate + the parity vectors.

export type PullRequestDraftContext = {
  branch: string | null
  base: string
  branchChangedByPreparation: boolean
  currentTitle: string
  currentBody: string
  currentDraft: boolean
  commitSummary: string
  changeSummary: string
  patch: string
}

export type GeneratedPullRequestFields = {
  base: string
  title: string
  body: string
  draft: boolean
}
