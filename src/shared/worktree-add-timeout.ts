// Why: bound `git worktree add` and the deferred git-crypt checkout so a
// OneDrive/cloud-placeholder stall fails fast (STA-1292, #7410) into rollback
// instead of hanging; generous enough for a legit large checkout (#7225).
// Shared so the local (src/main/git) and relay (src/relay) twins stay in sync.
export const WORKTREE_ADD_TIMEOUT_MS = 180_000
