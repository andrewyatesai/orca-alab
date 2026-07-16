import { createRequire } from 'node:module'
import { existsSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { setOrcaDispatchBinding } from '../shared/orca-dispatch-seam'

// The CLI is a plain Node (CommonJS) process, so it binds the shared dispatch
// seam via the SAME napi addon the main process uses — Node-API is ABI-stable, so
// one orca_node.node serves Electron and plain Node. (The relay/renderer wasm path
// is unavailable here: the wasm-bindgen glue is ESM and the CLI compiles to
// CommonJS.) Bound once at CLI entry so src/shared modules cut over to Rust reach
// the core. Binding is best-effort: if the addon can't be found the seam stays
// unbound and only the few dispatch-backed helpers (via requireOrcaDispatch) fail,
// not every command.

type OrcaDispatchAddon = {
  orcaDispatch(module: string, fn: string, inputJson: string): string
}

// The compiled binding lives at out/cli/orca-dispatch-binding.js, two levels below
// the repo/app root that holds native/orca-node — resolve the addon relative to
// this file (robust to the caller's cwd), with an env override and a cwd fallback.
function candidateAddonPaths(): string[] {
  const here = dirname(__filename)
  const paths: string[] = []
  const override = process.env.ORCA_RUST_GIT_ADDON ?? process.env.ORCA_RUST_TERMINAL_ADDON
  if (override) {
    paths.push(override)
  }
  paths.push(join(here, '..', '..', 'native', 'orca-node', 'orca_node.node'))
  paths.push(join(process.cwd(), 'native', 'orca-node', 'orca_node.node'))
  const resourcesPath = (process as { resourcesPath?: string }).resourcesPath
  if (resourcesPath) {
    paths.push(join(resourcesPath, 'orca_node.node'))
  }
  return paths
}

let bound = false

/** Bind the CLI's napi orcaDispatch into the shared seam. Idempotent; call once at
 *  entry before any dispatch-backed shared module runs. */
export function bindCliOrcaDispatch(): void {
  if (bound) {
    return
  }
  const nodeRequire = createRequire(__filename)
  for (const path of candidateAddonPaths()) {
    if (!existsSync(path)) {
      continue
    }
    try {
      const addon = nodeRequire(path) as OrcaDispatchAddon
      if (addon && typeof addon.orcaDispatch === 'function') {
        setOrcaDispatchBinding((module, fn, inputJson) => addon.orcaDispatch(module, fn, inputJson))
        bound = true
        return
      }
    } catch {
      // A bad/incompatible addon must not break the whole CLI; try the next path.
    }
  }
}
