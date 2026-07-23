// Why: git status is capped at this many changed-file entries. A repo with an
// enormous un-ignored folder can otherwise emit a listing large enough to crash
// the process when buffered. When the cap is hit the source-control view shows a
// "too many changes" state instead of the full list. Shared so the local path,
// the relay/SSH path, and the renderer agree on the same threshold.
export const DEFAULT_GIT_STATUS_LIMIT = 10_000

// Why: a bad limit (negative/fractional/NaN) breaks early-stop; require a valid
// non-negative int (0 disables the cap). Shared so every status path resolves it
// the same way instead of re-implementing the guard (#9477).
export function resolveGitStatusLimit(value: unknown): number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0
    ? value
    : DEFAULT_GIT_STATUS_LIMIT
}

// Why: cap a materialized entry list (e.g. a submodule commit-range expansion)
// so a huge change set can't hand the renderer an unbounded array; carry forward
// a prior didHitLimit/statusLength so a capped inner scan stays flagged (#9477).
export function capGitStatusEntries<T>(
  entries: T[],
  limit: number,
  previous: { didHitLimit?: boolean; statusLength?: number } = {}
): { entries: T[]; didHitLimit?: true; statusLength?: number } {
  const exceededLimit = limit > 0 && entries.length > limit
  if (!exceededLimit && previous.didHitLimit !== true) {
    return { entries }
  }
  return {
    entries: exceededLimit ? entries.slice(0, limit) : entries,
    didHitLimit: true,
    statusLength: Math.max(previous.statusLength ?? 0, entries.length)
  }
}
