import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import type { SessionMeta } from './history-manager'

// Session metadata (meta.json) read/write, keyed by the session's history dir.
// Best-effort by design: a missing or malformed file reads as null, and an
// unreadable file skips the update — history persistence must never crash the
// daemon (callers fire-and-forget these).

export function readSessionMetaFromDir(dir: string): SessionMeta | null {
  try {
    return JSON.parse(readFileSync(join(dir, 'meta.json'), 'utf-8'))
  } catch {
    return null
  }
}

export function updateSessionMeta(dir: string, updates: Partial<SessionMeta>): void {
  const metaPath = join(dir, 'meta.json')
  let meta: SessionMeta
  try {
    meta = JSON.parse(readFileSync(metaPath, 'utf-8'))
  } catch {
    return
  }
  Object.assign(meta, updates)
  writeFileSync(metaPath, JSON.stringify(meta, null, 2))
}
