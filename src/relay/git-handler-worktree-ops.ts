import * as path from 'node:path'
import {
  findGitCryptStateDirectory,
  shareGitCryptStateWithWorktree
} from '../shared/git-crypt-worktree-state'
import { resolveWorktreeAddBaseRef } from '../shared/worktree-base-ref'
import { WORKTREE_ADD_TIMEOUT_MS } from '../shared/worktree-add-timeout'
import type { GitExec } from './git-handler-ops'
export { removeWorktreeOp } from './git-handler-worktree-remove'
export { readRelayWorktreeList } from './git-handler-worktree-list'

async function rollbackRelayWorktreeCreate(
  git: GitExec,
  repoPath: string,
  targetDir: string,
  branchName: string,
  deleteBranch: boolean,
  worktreeKnownRegistered: boolean,
  error: unknown
): Promise<never> {
  const wrapped = error instanceof Error ? error : new Error(String(error))
  // Why: steps stay independently best-effort — a failed remove must not skip the prune that clears a stale admin record, nor the fresh-branch delete.
  const failedSteps: string[] = []
  let removed = true
  try {
    await git(['worktree', 'remove', '--force', targetDir], repoPath)
  } catch {
    removed = false
    failedSteps.push('worktree remove')
  }
  try {
    await git(['worktree', 'prune'], repoPath)
  } catch {
    failedSteps.push('worktree prune')
  }
  // Why: on unknown registration a failed remove may mean the add never created the branch, so -D could destroy a pre-existing one.
  if (deleteBranch && (removed || worktreeKnownRegistered)) {
    try {
      await git(['branch', '-D', '--', branchName], repoPath)
    } catch {
      failedSteps.push('branch delete')
    }
  }
  if (failedSteps.length > 0) {
    wrapped.message = `${wrapped.message} (cleanup also failed: ${failedSteps.join(', ')} — the partially created worktree at "${targetDir}" may need manual removal)`
  }
  throw wrapped
}

type RelayWorktreeAddRegistration = 'registered' | 'unregistered' | 'unknown'

async function probeRelayWorktreeAddRegistration(
  git: GitExec,
  repoPath: string,
  targetDir: string
): Promise<RelayWorktreeAddRegistration> {
  try {
    // Why: plain --porcelain (no -z) keeps this probe Git 2.25-safe; a registration check doesn't need newline-exact paths.
    const { stdout } = await git(['worktree', 'list', '--porcelain'], repoPath)
    return stdout
      .split('\n')
      .filter((line) => line.startsWith('worktree '))
      .some((line) => areRelayWorktreePathsEqual(line.slice('worktree '.length), targetDir))
      ? 'registered'
      : 'unregistered'
  } catch {
    // Why: state unknown after the failed add — err toward rollback so a registered orphan isn't silently kept.
    return 'unknown'
  }
}

async function persistRelayWorktreeCreationBase(
  git: GitExec,
  targetDir: string,
  branchName: string,
  effectiveBase: string
): Promise<void> {
  const configKey = `branch.${branchName}.base`
  try {
    await git(['config', '--local', '--replace-all', configKey, effectiveBase], targetDir)
  } catch (error) {
    console.warn(`relay addWorktree: failed to set ${configKey} for ${targetDir}`, error)
    try {
      // Why: SSH worktree creation shares branch config by name; clear stale
      // metadata if replacing an old same-name base fails.
      await git(['config', '--local', '--unset-all', configKey], targetDir)
    } catch (unsetError) {
      console.warn(
        `relay addWorktree: failed to unset stale ${configKey} for ${targetDir}`,
        unsetError
      )
    }
  }
}

export async function addWorktreeOp(git: GitExec, params: Record<string, unknown>): Promise<void> {
  const repoPath = params.repoPath as string
  const branchName = params.branchName as string
  const targetDir = params.targetDir as string
  const base = params.base as string | undefined
  const checkoutExistingBranch = params.checkoutExistingBranch === true
  const noCheckout = params.noCheckout === true

  // Why: a branchName starting with '-' would be interpreted as a git flag,
  // potentially changing the command's semantics (e.g. "--detach").
  if (branchName.startsWith('-') || (base && base.startsWith('-'))) {
    throw new Error('Branch name and base ref must not start with "-"')
  }

  // Why: --no-track + push.autoSetupRemote=true mirrors the local
  // addWorktree path (src/main/git/worktree.ts). Keeping the SSH path in
  // sync prevents a transport-only divergence where "Orca creates a
  // worktree" produces a different `git status` / `git push` UX based on
  // whether the repo is local or SSH-mounted. See full design rationale
  // (state machine, common-dir scope, old-git fallback) in the comments
  // around src/main/git/worktree.ts addWorktree — those invariants apply
  // identically here.
  const effectiveBase =
    base && !checkoutExistingBranch
      ? await resolveWorktreeAddBaseRef(base, async (qualifiedRef) => {
          try {
            await git(['rev-parse', '--verify', '--quiet', `${qualifiedRef}^{commit}`], repoPath)
            return true
          } catch {
            return false
          }
        })
      : undefined

  // Why: git-crypt resolves state through each worktree's private Git dir;
  // defer checkout until that dir references the repository-wide state.
  const gitCryptDir = await findGitCryptStateDirectory(git, repoPath)
  const deferCheckoutForGitCrypt = gitCryptDir !== null && !noCheckout

  // Why: Git 2.32+ on the SSH host overlaps checkout writes across cores;
  // older versions ignore the unknown config key (mirrors local addWorktree).
  const parallelCheckout = ['-c', 'checkout.workers=0']
  const args = [...parallelCheckout, 'worktree', 'add']
  if (noCheckout || deferCheckoutForGitCrypt) {
    args.push('--no-checkout')
  }
  if (checkoutExistingBranch) {
    args.push(targetDir, branchName)
  } else {
    args.push('--no-track', '-b', branchName, targetDir)
  }
  if (effectiveBase) {
    args.push(effectiveBase)
  }

  try {
    // Why: bound the add so a cloud-placeholder stall on the SSH host fails fast into rollback, mirroring local (STA-1292, #7410).
    await git(args, repoPath, { timeout: WORKTREE_ADD_TIMEOUT_MS })
  } catch (error) {
    // Why: a killed add (e.g. timeout mid-checkout, #7410) can leave a registered worktree + fresh branch; roll back only state this add created.
    const registration = await probeRelayWorktreeAddRegistration(git, repoPath, targetDir)
    if (registration === 'unregistered') {
      throw error
    }
    return rollbackRelayWorktreeCreate(
      git,
      repoPath,
      targetDir,
      branchName,
      !checkoutExistingBranch,
      registration === 'registered',
      error
    )
  }

  if (gitCryptDir) {
    try {
      await shareGitCryptStateWithWorktree(git, gitCryptDir, targetDir)
      if (deferCheckoutForGitCrypt) {
        // Why: bound the deferred git-crypt checkout too — same stall class, same fail-fast-into-rollback (mirrors local).
        await git([...parallelCheckout, 'checkout'], targetDir, {
          timeout: WORKTREE_ADD_TIMEOUT_MS
        })
      }
    } catch (error) {
      // Why: the add succeeded, so registration (and the fresh branch) is certain.
      return rollbackRelayWorktreeCreate(
        git,
        repoPath,
        targetDir,
        branchName,
        !checkoutExistingBranch,
        true,
        error
      )
    }
  }

  if (checkoutExistingBranch) {
    return
  }

  if (effectiveBase) {
    await persistRelayWorktreeCreationBase(git, targetDir, branchName, effectiveBase)
  }

  // Why: best-effort write so a deliberate user value (any scope) is
  // preserved and a real read failure is not silently overwritten. Final
  // catch is warn-only — if the remote host's git is <2.37 the value is
  // ignored at push time and the user falls back to `git push -u` once.
  // (Note: it is the SSH host's git that matters here, not the client's.)
  // Mirrors local addWorktree exactly.
  try {
    let alreadySet = false
    try {
      await git(['config', '--get', 'push.autoSetupRemote'], targetDir)
      alreadySet = true
    } catch (readError) {
      // Why: `git config --get` exits 1 only when the key is unset at every
      // scope. Any other code is a real read failure (corrupt config,
      // locked file) — surface it via the outer catch instead of falling
      // through to overwrite the user's actual value.
      const code = (readError as { code?: unknown })?.code
      if (code !== 1) {
        throw readError
      }
    }
    if (!alreadySet) {
      await git(['config', '--local', 'push.autoSetupRemote', 'true'], targetDir)
    }
  } catch (error) {
    console.warn(`relay addWorktree: failed to set push.autoSetupRemote for ${targetDir}`, error)
  }
}

function isPosixAbsolutePath(value: string): boolean {
  return value.startsWith('/')
}

function isWindowsAbsolutePath(value: string): boolean {
  return /^[A-Za-z]:[\\/]/.test(value) || value.startsWith('\\\\')
}

function normalizeRelayWorktreePathForCompare(value: string): string {
  if (isPosixAbsolutePath(value)) {
    return path.posix.normalize(path.posix.resolve(value))
  }
  if (isWindowsAbsolutePath(value)) {
    return path.win32.normalize(path.win32.resolve(value))
  }
  return path.normalize(path.resolve(value))
}

export function areRelayWorktreePathsEqual(leftPath: string, rightPath: string): boolean {
  const left = normalizeRelayWorktreePathForCompare(leftPath)
  const right = normalizeRelayWorktreePathForCompare(rightPath)
  const compareCaseInsensitive = isWindowsAbsolutePath(leftPath) && isWindowsAbsolutePath(rightPath)
  return compareCaseInsensitive ? left.toLowerCase() === right.toLowerCase() : left === right
}

export async function worktreeIsCleanOp(
  git: GitExec,
  params: Record<string, unknown>
): Promise<{ clean: boolean; stdout?: string }> {
  const worktreePath = params.worktreePath as string
  const includeUntracked = params.includeUntracked !== false
  const { stdout } = await git(
    ['status', '--porcelain', includeUntracked ? '--untracked-files=all' : '--untracked-files=no'],
    worktreePath
  )
  const clean = !stdout.trim()
  return { clean, stdout: clean ? undefined : stdout }
}

export async function commitChangesRelay(
  git: GitExec,
  worktreePath: string,
  message: string
): Promise<{ success: boolean; error?: string }> {
  // Why: defense-in-depth. The IPC handler at src/main/ipc/filesystem.ts validates
  // the message, but a relay caller (future automation, or an SSH client connecting
  // to the relay directly) could bypass that path. Reject empty/whitespace messages
  // here so we surface a clear error instead of git's opaque failure.
  if (typeof message !== 'string' || message.trim().length === 0) {
    return { success: false, error: 'Commit message is required' }
  }

  try {
    await git(['commit', '-m', message], worktreePath)
    return { success: true }
  } catch (error) {
    // Why: surface whichever channel carries the useful message. Pre-commit/GPG
    // hook failures write to stderr; "nothing to commit, working tree clean"
    // writes to stdout. Try stderr first, fall back to stdout, then error.message.
    // Mirrors commitChanges in src/main/git/status.ts — keep the two paths in sync.
    const readStringField = (field: string): string | null => {
      if (typeof error === 'object' && error && field in error) {
        const v = (error as Record<string, unknown>)[field]
        if (typeof v === 'string' && v.length > 0) {
          return v
        }
      }
      return null
    }
    const errorMessage =
      readStringField('stderr') ??
      readStringField('stdout') ??
      (error instanceof Error ? error.message : 'Commit failed')
    return { success: false, error: errorMessage }
  }
}
