import { randomUUID } from 'node:crypto'
import { copyFileSync, renameSync, rmSync, writeFileSync } from 'node:fs'
import { dirname } from 'node:path'
import { grantDirAcl, isPermissionError } from '../win32-utils'

// Why: #9582 CAS — signals a caller-supplied revalidation guard aborted the replace because a
// concurrent writer now owns the target; callers re-run rather than clobber the fresh contents.
export class ConcurrentReplaceConflictError extends Error {
  constructor() {
    super('Concurrent writer changed the replace target; aborting to avoid clobbering it')
    this.name = 'ConcurrentReplaceConflictError'
  }
}

export function writeFileAtomically(
  targetPath: string,
  contents: string,
  options?: { mode?: number; revalidateBeforeReplace?: () => boolean }
): void {
  const tmpPath = `${targetPath}.${process.pid}.${randomUUID()}.tmp`
  try {
    writeFileSync(tmpPath, contents, { encoding: 'utf-8', mode: options?.mode })
    renameFileWithWindowsRetry(tmpPath, targetPath, options?.revalidateBeforeReplace)
  } catch (error) {
    rmSync(tmpPath, { force: true })
    // Why: a revalidation abort is not a filesystem failure; surface it without the ACL retry so
    // the caller re-runs instead of grinding on a rename it deliberately declined.
    if (error instanceof ConcurrentReplaceConflictError) {
      throw error
    }
    // Why: on Windows, Chromium's renderer initialization calls
    // SetNamedSecurityInfo on the userData folder with a Protected DACL
    // that propagates empty inherited ACEs to child directories, causing
    // EPERM on all writes. Grant an explicit ACL on the parent directory
    // and retry once so the write succeeds even if Chromium reset the DACL
    // after our startup fix ran.
    if (isPermissionError(error) && process.platform === 'win32') {
      try {
        grantDirAcl(dirname(targetPath))
        const retryTmpPath = `${targetPath}.${process.pid}.${randomUUID()}.tmp`
        try {
          writeFileSync(retryTmpPath, contents, { encoding: 'utf-8', mode: options?.mode })
          renameFileWithWindowsRetry(retryTmpPath, targetPath, options?.revalidateBeforeReplace)
          return
        } catch (retryError) {
          rmSync(retryTmpPath, { force: true })
          if (retryError instanceof ConcurrentReplaceConflictError) {
            throw retryError
          }
        }
      } catch (aclError) {
        // Why: a revalidation abort during the ACL retry must still propagate, not be swallowed as
        // an unactionable icacls failure that re-throws the original EPERM.
        if (aclError instanceof ConcurrentReplaceConflictError) {
          throw aclError
        }
        // icacls failure is not actionable; re-throw the original EPERM
      }
    }
    throw error
  }
}

// Why: on Windows, file replacement and backup-copy operations can fail with
// EPERM/EACCES/EBUSY if another process (antivirus, Claude CLI, Codex CLI)
// holds the target file open. A short retry avoids transient failures without
// masking real permission errors. Total backoff (~750ms) covers typical AV
// scan windows seen in issue #1507.
export function renameFileWithWindowsRetry(
  source: string,
  target: string,
  revalidateBeforeReplace?: () => boolean
): void {
  runFileOperationWithWindowsRetry(() => renameSync(source, target), revalidateBeforeReplace)
}

export function copyFileWithWindowsRetry(source: string, target: string): void {
  runFileOperationWithWindowsRetry(() => copyFileSync(source, target))
}

function runFileOperationWithWindowsRetry(
  operation: () => void,
  revalidateBeforeReplace?: () => boolean
): void {
  const maxAttempts = process.platform === 'win32' ? 6 : 1
  for (let attempt = 1; attempt <= maxAttempts; attempt++) {
    // Why: #9582 — revalidate the CAS baseline before every replace attempt, not just once up front;
    // on Windows the rename retries for ~750ms and a concurrent Codex refresh landing mid-backoff
    // would otherwise be clobbered by the later successful rename.
    if (revalidateBeforeReplace && !revalidateBeforeReplace()) {
      throw new ConcurrentReplaceConflictError()
    }
    try {
      operation()
      return
    } catch (error) {
      const code = (error as NodeJS.ErrnoException).code
      if (attempt < maxAttempts && (code === 'EPERM' || code === 'EACCES' || code === 'EBUSY')) {
        sleepSync(attempt * 50)
        continue
      }
      throw error
    }
  }
}

// Why: writeFileAtomically is a sync API called from sync paths, so the retry
// backoff must park the thread instead of burning CPU in a Date.now() loop.
const sleepBuffer = new Int32Array(new SharedArrayBuffer(4))
function sleepSync(ms: number): void {
  Atomics.wait(sleepBuffer, 0, 0, ms)
}
