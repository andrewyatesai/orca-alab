import { createRequire } from 'node:module'
import { join } from 'node:path'

// Candidate-path policy for the orca_node.node napi addon, shared by
// rust-terminal-addon.ts and rust-git-addon.ts so both loaders resolve the
// binary identically.

export type OrcaNodeAddonPathContext = {
  /** Explicit env override (already resolved by the caller), probed first. */
  override?: string
  /** Electron `app.isPackaged` (false under plain Node / `pnpm dev`). */
  isPackaged: boolean
  cwd: string
  resourcesPath?: string
}

/** True only inside a packaged Electron build. Read defensively: these modules
 *  also load under plain Node (tests, daemon), where electron is unavailable. */
export function isPackagedElectronProcess(): boolean {
  if (!process.versions.electron) {
    return false
  }
  try {
    const { app } = createRequire(import.meta.url)('electron') as {
      app?: { isPackaged?: boolean }
    }
    return app?.isPackaged === true
  } catch {
    return false
  }
}

/** Candidate orca_node.node paths, most-preferred first. Packaged builds probe
 *  only the override and resourcesPath — never cwd, so a stale dev-built addon
 *  under the launch directory cannot silently shadow the shipped engine
 *  (mirrors daemon-init.ts's packaged-only daemon-binary resolution). */
export function orcaNodeAddonCandidatePaths(ctx: OrcaNodeAddonPathContext): string[] {
  const paths: string[] = []
  if (ctx.override) {
    paths.push(ctx.override)
  }
  if (!ctx.isPackaged) {
    // Dev tree layout: <repo>/native/orca-node/orca_node.node. process.cwd()
    // is the repo root under `pnpm dev`.
    paths.push(join(ctx.cwd, 'native', 'orca-node', 'orca_node.node'))
  }
  // Packaged layout: alongside other unpacked native resources.
  if (ctx.resourcesPath) {
    paths.push(join(ctx.resourcesPath, 'orca_node.node'))
  }
  return paths
}
