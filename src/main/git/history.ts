import type { GitHistoryOptions, GitHistoryResult } from '../../shared/git-history'
import { loadGitHistoryFromExecutor } from '../../shared/git-history'
import type { GitRuntimeOptions } from './git-runtime-options'
import { gitOptionsForWorktree } from './git-runtime-options'
import { gitExecFileAsync } from './runner'
import { parseGitHistoryLogNative } from './rust-git-history-log-parser'

export async function getHistory(
  worktreePath: string,
  options: GitHistoryOptions & GitRuntimeOptions = {}
): Promise<GitHistoryResult> {
  return loadGitHistoryFromExecutor(
    (args, cwd) => gitExecFileAsync(args, gitOptionsForWorktree(cwd, options)),
    worktreePath,
    options,
    // Route log parsing through the verified Rust parser in the main process.
    parseGitHistoryLogNative
  )
}
