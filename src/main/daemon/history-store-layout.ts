import { join } from 'node:path'
import {
  chmodSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmSync
} from 'node:fs'
import { hardenSecurePath } from '../../shared/secure-file'

/**
 * On-disk layout of the daemon session-history store.
 *
 * Daemon session dirs (checkpoint.json / output.log / meta.json) live in a
 * daemon-owned subdirectory of userData/terminal-history instead of sharing
 * its top level with the shell-HISTFILE store. The two stores previously
 * only avoided deleting each other's data by accidental meta.json shape
 * differences; a path-level namespace makes the boundary explicit so either
 * meta format can grow fields safely.
 */
export const DAEMON_SESSIONS_DIR_NAME = 'daemon-sessions'

// Why 0o700/0o600: scrollback routinely contains secrets (echoed tokens, env
// dumps). Match the discipline of the sibling stores (daemon token, shell
// HISTFILE dirs, scrollback snapshots) instead of umask defaults.
export const HISTORY_DIR_MODE = 0o700
export const HISTORY_FILE_MODE = 0o600

/** Pure path join — for callers (sweeps) that must not create directories. */
export function getDaemonSessionStoreRoot(terminalHistoryRoot: string): string {
  return join(terminalHistoryRoot, DAEMON_SESSIONS_DIR_NAME)
}

type LegacyMetaShape = {
  worktreeId?: unknown
  startedAt?: unknown
}

function readMetaShape(dir: string): LegacyMetaShape | null {
  try {
    const parsed = JSON.parse(readFileSync(join(dir, 'meta.json'), 'utf-8'))
    return typeof parsed === 'object' && parsed !== null ? (parsed as LegacyMetaShape) : null
  } catch {
    return null
  }
}

// Why a shape test and not a name test: daemon dirs are encodeURIComponent
// session ids and shell-HISTFILE dirs are 16-hex hashes, but the meta.json
// shape is the documented discriminator — daemon SessionMeta has startedAt
// and never worktreeId; the shell store's meta is exactly the inverse.
function isDaemonSessionDir(dir: string): boolean {
  const meta = readMetaShape(dir)
  if (meta) {
    return typeof meta.startedAt === 'string' && meta.worktreeId === undefined
  }
  // Corrupt/missing meta: fall back to daemon-only artifact names so damaged
  // session dirs still migrate (and later age out via GC) instead of lingering
  // in the shared root forever.
  return (
    existsSync(join(dir, 'checkpoint.json')) ||
    existsSync(join(dir, 'output.log')) ||
    existsSync(join(dir, 'scrollback.bin'))
  )
}

/**
 * Move daemon session dirs created by older builds from the shared
 * terminal-history root into the daemon-owned subdirectory. Idempotent: a
 * second run finds nothing daemon-shaped at the top level. Failures are
 * per-entry and non-fatal (retried on the next startup).
 */
export function migrateLegacyDaemonSessionDirs(
  terminalHistoryRoot: string,
  sessionsRoot: string
): number {
  let moved = 0
  let entries: { isDirectory(): boolean; name: string }[]
  try {
    entries = readdirSync(terminalHistoryRoot, { withFileTypes: true })
  } catch {
    return moved
  }
  for (const entry of entries) {
    if (!entry.isDirectory() || entry.name === DAEMON_SESSIONS_DIR_NAME) {
      continue
    }
    const source = join(terminalHistoryRoot, entry.name)
    try {
      if (!isDaemonSessionDir(source)) {
        continue
      }
      const target = join(sessionsRoot, entry.name)
      if (existsSync(target)) {
        // Why: the target can only exist if a newer build already recreated
        // the session in the new layout — the stranded legacy copy is stale.
        rmSync(source, { recursive: true, force: true })
      } else {
        renameSync(source, target)
      }
      moved++
    } catch (err) {
      console.warn(
        `[history:layout] failed to migrate ${entry.name}: ${err instanceof Error ? err.message : String(err)}`
      )
    }
  }
  return moved
}

/**
 * One-time tightening sweep for files written by older builds at umask
 * defaults. POSIX chmods the tree; Windows follows the secure-file ACL
 * pattern (docs/reference/windows-secure-file-acl-hardening.md): restrict the store
 * root asynchronously once and let NTFS inheritance cover children.
 */
export function tightenDaemonSessionStorePermissions(sessionsRoot: string): void {
  if (process.platform === 'win32') {
    hardenSecurePath(sessionsRoot, { isDirectory: true, platform: process.platform })
    return
  }
  try {
    chmodSync(sessionsRoot, HISTORY_DIR_MODE)
    for (const entry of readdirSync(sessionsRoot, { withFileTypes: true })) {
      const entryPath = join(sessionsRoot, entry.name)
      try {
        if (entry.isDirectory()) {
          chmodSync(entryPath, HISTORY_DIR_MODE)
          for (const file of readdirSync(entryPath)) {
            chmodSync(join(entryPath, file), HISTORY_FILE_MODE)
          }
        } else {
          chmodSync(entryPath, HISTORY_FILE_MODE)
        }
      } catch {
        // Per-entry, non-fatal — a vanished or unreadable entry must not
        // abort the sweep for the rest of the store.
      }
    }
  } catch (err) {
    console.warn(
      `[history:layout] permission sweep failed: ${err instanceof Error ? err.message : String(err)}`
    )
  }
}

const preparedRoots = new Set<string>()

/**
 * Resolve (and on first call per process: create, migrate, permission-tighten)
 * the daemon session store root under the given terminal-history root.
 */
export function prepareDaemonSessionStoreRoot(terminalHistoryRoot: string): string {
  const sessionsRoot = getDaemonSessionStoreRoot(terminalHistoryRoot)
  if (preparedRoots.has(sessionsRoot)) {
    return sessionsRoot
  }
  // Why mode on both levels: the shared terminal-history root was historically
  // created modeless by daemon init; recursive mkdir only applies the mode to
  // dirs it creates, so the tighten sweep below still fixes pre-existing ones.
  mkdirSync(sessionsRoot, { recursive: true, mode: HISTORY_DIR_MODE })
  if (process.platform !== 'win32') {
    try {
      chmodSync(terminalHistoryRoot, HISTORY_DIR_MODE)
    } catch {
      // Non-fatal: the subdir sweep below still protects the session files.
    }
  }
  migrateLegacyDaemonSessionDirs(terminalHistoryRoot, sessionsRoot)
  tightenDaemonSessionStorePermissions(sessionsRoot)
  preparedRoots.add(sessionsRoot)
  return sessionsRoot
}

export function __resetHistoryStoreLayoutForTests(): void {
  preparedRoots.clear()
}
