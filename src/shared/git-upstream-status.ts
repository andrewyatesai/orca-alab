import type { GitUpstreamStatus } from './git-status-types'

// The `git cherry` mark-output parser (upstreamOnlyCommitsArePatchEquivalent)
// moved to the Rust orca-core git_upstream_status module: the relay drives it
// via wasm (src/relay/git-wasm.ts); main's equivalent runs inside the Rust
// upstream/push flows via the napi A-bridge. This shared module keeps only the
// typed-object predicate below — object-field logic the compiler pins, not
// drift-prone output parsing.

export function shouldForcePushWithLeaseForUpstream(
  status: GitUpstreamStatus | undefined
): boolean {
  return (
    status?.hasUpstream === true &&
    status.ahead > 0 &&
    status.behind > 0 &&
    status.behindCommitsArePatchEquivalent === true
  )
}
