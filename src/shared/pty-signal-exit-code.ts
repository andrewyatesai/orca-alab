/**
 * Collapses a node-pty exit event into a single exit code.
 *
 * Why: node-pty reports exitCode 0 for signal-terminated children (pty.cc only
 * sets it under WIFEXITED), so dropping `signal` makes SIGKILL/OOM/segfault
 * deaths read as clean exits downstream (dead-pane overlay, userEndedCleanly,
 * session status). Encode signaled exits as POSIX 128+signal instead.
 */
export function resolvePtyExitCode(exit: { exitCode: number; signal?: number }): number {
  return typeof exit.signal === 'number' && exit.signal > 0 ? 128 + exit.signal : exit.exitCode
}
