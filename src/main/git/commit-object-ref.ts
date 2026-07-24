import { gitExecFileAsync } from './runner'

type GitExec = (args: string[]) => Promise<unknown>

// Why: accept both SHA-1 (40 hex) and SHA-256 (64 hex, git 2.29+) object ids — a
// SHA-256 repo's present commit was reported absent, firing a redundant remote fetch.
// The rev-parse `^{commit}` probe still fails closed for anything that isn't a commit.
const FULL_GIT_OBJECT_ID_PATTERN = /^([0-9a-f]{40}|[0-9a-f]{64})$/i

export function isFullGitObjectId(value: string): boolean {
  return FULL_GIT_OBJECT_ID_PATTERN.test(value.trim())
}

export async function hasCommitObjectViaGitExec(gitExec: GitExec, ref: string): Promise<boolean> {
  const candidate = ref.trim()
  if (!isFullGitObjectId(candidate)) {
    return false
  }
  try {
    await gitExec(['rev-parse', '--verify', '--quiet', `${candidate}^{commit}`])
    return true
  } catch {
    return false
  }
}

export function hasLocalCommitObject(repoPath: string, ref: string): Promise<boolean> {
  return hasCommitObjectViaGitExec((args) => gitExecFileAsync(args, { cwd: repoPath }), ref)
}
