import { createRequire } from 'module'
import { join } from 'path'
import { existsSync } from 'fs'

// Typed surface of the napi addon built from native/orca-node (the Rust
// `orca_terminal::HeadlessTerminal`). Node-API is ABI-stable, so the same
// .node loads in both plain Node and Electron without an electron-rebuild.

export type RustHeadlessTerminalHandle = {
  write(data: Buffer): void
  resize(cols: number, rows: number): void
  snapshot(): string[]
  scrollbackLen(): number
  clearScrollback(): void
  cwd(): string | null
  cursor(): number[]
  mouseTracking(): string
  sgrMouse(): boolean
  sgrPixels(): boolean
  isAlternateScreen(): boolean
  bracketedPaste(): boolean
  applicationCursor(): boolean
  serializeAnsi(): string
}

export type RustHeadlessTerminalCtor = new (
  cols: number,
  rows: number,
  scrollback?: number
) => RustHeadlessTerminalHandle

export type RustTerminalBinding = {
  HeadlessTerminal: RustHeadlessTerminalCtor
  engine(): string
}

function candidatePaths(): string[] {
  const paths: string[] = []
  const override = process.env.ORCA_RUST_TERMINAL_ADDON
  if (override) {
    paths.push(override)
  }
  // Dev tree layout: <repo>/native/orca-node/orca_node.node. process.cwd() is
  // the repo root under `pnpm dev`.
  paths.push(join(process.cwd(), 'native', 'orca-node', 'orca_node.node'))
  // Packaged layout: alongside other unpacked native resources. resourcesPath
  // is Electron-only, so read it defensively rather than via the global type.
  const resourcesPath = (process as { resourcesPath?: string }).resourcesPath
  if (resourcesPath) {
    paths.push(join(resourcesPath, 'orca_node.node'))
  }
  return paths
}

let cached: RustTerminalBinding | null | undefined

/** Load the Rust terminal addon, or return null if it is unavailable or fails
 *  to load. Never throws — callers fall back to the TypeScript emulator. */
export function loadRustTerminalBinding(): RustTerminalBinding | null {
  if (cached !== undefined) {
    return cached
  }
  const req = createRequire(import.meta.url)
  for (const path of candidatePaths()) {
    if (!existsSync(path)) {
      continue
    }
    try {
      const binding = req(path) as RustTerminalBinding
      if (binding && typeof binding.HeadlessTerminal === 'function') {
        cached = binding
        return cached
      }
    } catch {
      // try the next candidate; a bad/incompatible addon must not break startup
    }
  }
  cached = null
  return cached
}
