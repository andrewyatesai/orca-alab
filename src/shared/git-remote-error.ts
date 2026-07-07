// The git remote-error text logic (normalizeGitErrorMessage,
// stripCredentialsFromMessage, isNoUpstreamError,
// formatSubmodulePushFailureDetail) moved to the Rust orca-text core: the main
// process drives it via napi (src/main/git/rust-git-remote-error.ts), the
// relay via wasm (src/relay/git-wasm.ts), and the renderer via wasm
// (src/renderer/src/lib/git-wasm/git-remote-error.ts). This shared module
// keeps only the operation type used at those JS boundaries.
export type GitRemoteOperation = 'push' | 'pull' | 'fetch' | 'upstream'
