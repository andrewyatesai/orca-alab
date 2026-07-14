import type { SetupScriptImportProvider } from './setup-script-import-providers'
import { requireOrcaDispatch } from './orca-dispatch-seam'

export type SetupScriptImportCandidate = {
  provider: SetupScriptImportProvider
  label: string
  files: string[]
  setup: string
  archive?: string
  unsupportedFields?: string[]
}

export type SetupScriptImportFileRead = (relativePath: string) => Promise<string | null>
export type SetupScriptImportFileExists = (relativePath: string) => Promise<boolean>

// The config files whose CONTENT the Rust core parses. These path strings must
// stay in lockstep with `orca-config::setup_script_imports` (the core looks each
// up in the contentsByPath map the IO edge supplies below).
const SETUP_SCRIPT_CONTENT_PATHS = [
  '.superset/config.json',
  '.superset/config.local.json',
  'conductor.json',
  '.codex/environments/environment.toml',
  '.cmux/cmux.json',
  'cmux.json',
  'package.json'
] as const

// Lockfiles whose EXISTENCE the package-manager provider checks. With an injected
// fileExists (stat-based) they cross as `existingPaths`; without one the core
// falls back to read-based existence, so they're read into the map instead.
const PACKAGE_MANAGER_LOCKFILE_PATHS = [
  'pnpm-lock.yaml',
  'bun.lock',
  'bun.lockb',
  'yarn.lock',
  'package-lock.json',
  'npm-shrinkwrap.json'
] as const

// Setup-script-import inspection is cut over to the Rust orca-config core via the
// orcaDispatch aggregate (main/runtime readers only). The IO edge — reading the
// candidate files (local fs or SSH) — stays in TS and crosses as a content map;
// the pure JSON/TOML parsing + provider derivation runs in Rust. The reader
// contract is preserved so callers (orca-runtime.ts / worktrees.ts) don't change.
export async function inspectSetupScriptImportCandidates(
  readFile: SetupScriptImportFileRead,
  options?: { fileExists?: SetupScriptImportFileExists }
): Promise<SetupScriptImportCandidate[]> {
  const fileExists = options?.fileExists
  // Without an injected existence check the core falls back to read-based
  // lockfile existence, so read the lockfiles into the map in that case.
  const readPaths = fileExists
    ? SETUP_SCRIPT_CONTENT_PATHS
    : [...SETUP_SCRIPT_CONTENT_PATHS, ...PACKAGE_MANAGER_LOCKFILE_PATHS]
  const contentEntries = await Promise.all(
    readPaths.map(async (path) => [path, await readFile(path)] as const)
  )
  const contentsByPath: Record<string, string | null> = Object.fromEntries(contentEntries)

  const input: { contentsByPath: Record<string, string | null>; existingPaths?: string[] } = {
    contentsByPath
  }
  if (fileExists) {
    const existence = await Promise.all(
      PACKAGE_MANAGER_LOCKFILE_PATHS.map(async (path) => [path, await fileExists(path)] as const)
    )
    input.existingPaths = existence.filter(([, exists]) => exists).map(([path]) => path)
  }

  return requireOrcaDispatch(
    'setup-script-imports',
    'inspectSetupScriptImportCandidates',
    input
  ) as SetupScriptImportCandidate[]
}
