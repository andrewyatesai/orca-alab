// TS dispatch for the git-history-log-parser parity module: maps the shared
// vector function names to the real `src/shared/git-history-log-parser.ts`
// exports so the harness compares the live TS reference against the Rust port.

import {
  gitHistoryRefFromFullName,
  parseGitHistoryLog,
  shortGitHash
} from '../../../src/shared/git-history-log-parser'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'parseGitHistoryLog':
      return parseGitHistoryLog(input as string)
    case 'shortGitHash':
      return shortGitHash(input as string)
    case 'gitHistoryRefFromFullName': {
      const { fullName, fallbackName, revision } = input as {
        fullName: string | null
        fallbackName: string
        revision: string
      }
      return gitHistoryRefFromFullName(fullName, fallbackName, revision)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
