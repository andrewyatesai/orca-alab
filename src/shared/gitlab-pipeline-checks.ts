// Impl DELETED — the Rust orca-core `gitlab_pipeline_checks` port is the sole
// implementation. Main drives the status mappers via napi
// (src/main/rust-gitlab-pipeline-checks.ts) and the renderer drives the
// job → check-row mapping via the orca-git wasm
// (src/renderer/src/lib/git-wasm/gitlab-pipeline-checks.ts); parity pins that
// surface. Only the boundary types remain, re-exported so consumers keyed off
// this module path keep resolving without a napi/wasm import in src/shared.
export type { GitLabPipelineJob } from './gitlab-types'
export type { PRCheckDetail } from './types'
