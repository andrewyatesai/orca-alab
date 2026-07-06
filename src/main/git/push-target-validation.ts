import type { GitPushTarget } from '../../shared/types'
import { assertGitPushTargetShapeNative } from './rust-push-target-validation'
import { gitExecFileAsync } from './runner'

type GitExecOptions = {
  wslDistro?: string
}

export async function validateGitPushTarget(
  repoPath: string,
  target: unknown,
  options: GitExecOptions = {}
): Promise<GitPushTarget> {
  // Rust owns the value-rule validation (path-traversal safety); git execution
  // of check-ref-format stays here in TS so SSH/WSL/env routing is preserved.
  assertGitPushTargetShapeNative(target)
  await gitExecFileAsync(['check-ref-format', '--branch', target.branchName], {
    cwd: repoPath,
    ...options
  })
  return target
}
