// Pure planning core for the daemon session-history GC. Lifted out of
// runDaemonSessionHistoryGc so the age-expiry + size-eviction DECISIONS are
// unit-testable and machine-checkable without a filesystem — the scan and the
// rmSync stay in the executor. This is the TS half of the `orca-session-gc` E1
// pair (proven equivalent to the Rust core by
// rust/crates/orca-session-gc/parity-corpus.txt, proven correct by its
// proofs/ay/*.smt2). Every bound is a privacy bound as much as a disk bound —
// scrollback is secret-bearing — so "never lose a recoverable live session" and
// "keep the store under budget" are the safety properties that matter.

/** A scanned session dir reduced to just the fields the plan depends on. */
export type SessionGcPlannerDir = {
  name: string
  totalBytes: number
  /** Newest mtime across the dir — "last activity". */
  lastActivityMs: number
  /** meta.endedAt is a non-null string (the dir can no longer cold-restore). */
  isEnded: boolean
}

export type SessionGcThresholds = {
  minDirAgeMs: number
  endedRetentionMs: number
  unrestoredRetentionMs: number
}

export type SessionGcPlan = {
  /** Names to delete for age (in scan order). */
  expire: string[]
  /** Names to delete for the size cap (in eviction order: oldest activity first). */
  evictForSize: string[]
  /** Store bytes remaining once every planned deletion succeeds. */
  remainingBytes: number
}

/**
 * Whether a scanned dir should be age-expired. Live dirs and dirs younger than
 * the TOCTOU floor are always exempt. Otherwise the retention threshold is: ended
 * dirs → `endedRetentionMs`; not-ended dirs → `unrestoredRetentionMs`, EXCEPT when
 * liveness is unknown, where a not-ended dir might belong to a live-but-unreattached
 * session and must never expire (∞ retention).
 */
export function shouldExpireSessionDir(input: {
  isLive: boolean
  ageMs: number
  isEnded: boolean
  livenessUnknown: boolean
  minDirAgeMs: number
  endedRetentionMs: number
  unrestoredRetentionMs: number
}): boolean {
  if (input.isLive || input.ageMs < input.minDirAgeMs) {
    return false
  }
  const retention = input.isEnded
    ? input.endedRetentionMs
    : input.livenessUnknown
      ? Number.POSITIVE_INFINITY
      : input.unrestoredRetentionMs
  return input.ageMs > retention
}

/**
 * Plan the age-expiry and size-cap eviction over a scanned session store. Pure:
 * the caller scans the fs into `dirs` and applies the returned names. Size-cap
 * eviction is oldest-first and restricted to "evictable" dirs — ended dirs always,
 * and not-ended dirs only when liveness is KNOWN (an unknown-liveness not-ended dir
 * might be a live session's recovery data, so it is never evicted for disk).
 */
export function planSessionHistoryGc(input: {
  dirs: SessionGcPlannerDir[]
  now: number
  maxTotalBytes: number
  /** liveDirNames === null in the caller. */
  livenessUnknown: boolean
  /** Decoded dir names known to be live; used only when liveness is known. */
  liveDirNames: ReadonlySet<string> | null
  thresholds: SessionGcThresholds
}): SessionGcPlan {
  const expire: string[] = []
  const evictionCandidates: SessionGcPlannerDir[] = []
  let survivorBytes = 0
  for (const dir of input.dirs) {
    const isLive = input.liveDirNames?.has(dir.name) ?? false
    const ageMs = input.now - dir.lastActivityMs
    const exempt = isLive || ageMs < input.thresholds.minDirAgeMs
    if (
      shouldExpireSessionDir({
        isLive,
        ageMs,
        isEnded: dir.isEnded,
        livenessUnknown: input.livenessUnknown,
        minDirAgeMs: input.thresholds.minDirAgeMs,
        endedRetentionMs: input.thresholds.endedRetentionMs,
        unrestoredRetentionMs: input.thresholds.unrestoredRetentionMs
      })
    ) {
      expire.push(dir.name)
      continue
    }
    survivorBytes += dir.totalBytes
    // Only non-exempt survivors are size-eviction candidates; live/recent dirs are
    // counted toward the total but never evicted.
    if (!exempt && (dir.isEnded || !input.livenessUnknown)) {
      evictionCandidates.push(dir)
    }
  }

  const evictForSize: string[] = []
  let remainingBytes = survivorBytes
  if (remainingBytes > input.maxTotalBytes) {
    const oldestFirst = [...evictionCandidates].sort((a, b) => a.lastActivityMs - b.lastActivityMs)
    for (const dir of oldestFirst) {
      if (remainingBytes <= input.maxTotalBytes) {
        break
      }
      remainingBytes -= dir.totalBytes
      evictForSize.push(dir.name)
    }
  }
  return { expire, evictForSize, remainingBytes }
}
