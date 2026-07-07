// TS dispatch for the setup-script-imports parity module: maps the shared
// vector function name to the real `src/shared/setup-script-imports.ts` entry
// so the harness compares the live TS reference against the Rust port
// (`orca-config::setup_script_imports` + codex-environment + package-manager).
//
// The entry takes async file readers, so this dispatcher returns a Promise;
// the parity driver awaits dispatcher results.

import { inspectSetupScriptImportCandidates } from '../../../src/shared/setup-script-imports'

type SetupScriptImportsInput = {
  contentsByPath?: Record<string, string | null>
  existingPaths?: string[]
}

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'inspectSetupScriptImportCandidates': {
      const { contentsByPath = {}, existingPaths } = input as SetupScriptImportsInput
      // Why: real readers (orca-runtime.ts / worktrees.ts inspectSetupScriptImports)
      // resolve null for missing/unreadable files and never throw.
      const readFile = async (relativePath: string): Promise<string | null> =>
        contentsByPath[relativePath] ?? null
      // Why: worktrees.ts passes a stat-based fileExists while orca-runtime.ts
      // omits it; an absent existingPaths exercises the read-fallback caller shape.
      const options = existingPaths
        ? {
            fileExists: async (relativePath: string): Promise<boolean> =>
              existingPaths.includes(relativePath)
          }
        : undefined
      return inspectSetupScriptImportCandidates(readFile, options)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
