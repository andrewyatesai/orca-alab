import type { GitPushTarget } from '../../shared/types'
import { assertGitPushTargetShapePreferRust } from './rust-push-target-validation'
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
  assertGitPushTargetShapePreferRust(target)
  await gitExecFileAsync(['check-ref-format', '--branch', target.branchName], {
    cwd: repoPath,
    ...options
  })
  return target
}
