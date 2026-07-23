import type { NormalizedOutputOptions, OutputBundle, OutputChunk, Plugin } from 'rollup'

// Why: v1.4.129-rc.1 shipped a dead terminal daemon because a shared main
// chunk gained `require("electron")` (an import edge added in #7642), and a
// plain-Node fork cannot require electron. This guard fails the build when any
// chunk reachable from a plain-Node fork entry requires electron.

// Entries executed as plain Node (ELECTRON_RUN_AS_NODE / no electron runtime):
// parcel-watcher and computer sidecars, and the CLI-run agent-hooks entry.
// require("electron") throws MODULE_NOT_FOUND in all of them.
const PLAIN_NODE_ENTRY_NAMES = [
  'parcel-watcher-process-entry',
  'computer-sidecar',
  'agent-hooks/managed-agent-hook-controls'
] as const

const ELECTRON_REQUIRE_RE = /require\(\s*["']electron["']\s*\)/

function collectReachableChunks(
  entry: OutputChunk,
  byFileName: Map<string, OutputChunk>
): OutputChunk[] {
  const seen = new Set<string>()
  const reachable: OutputChunk[] = []
  const stack = [entry.fileName]
  while (stack.length > 0) {
    const fileName = stack.pop() as string
    if (seen.has(fileName)) {
      continue
    }
    seen.add(fileName)
    const chunk = byFileName.get(fileName)
    if (!chunk) {
      continue
    }
    reachable.push(chunk)
    for (const imported of [...chunk.imports, ...chunk.dynamicImports]) {
      stack.push(imported)
    }
  }
  return reachable
}

function assertNoElectronRequire(
  entryName: string,
  entry: OutputChunk,
  byFileName: Map<string, OutputChunk>
): void {
  for (const chunk of collectReachableChunks(entry, byFileName)) {
    if (ELECTRON_REQUIRE_RE.test(chunk.code)) {
      throw new Error(
        `[plain-node-entry-guard] "${entryName}" reaches chunk "${chunk.fileName}" that ` +
          `requires electron. "${entryName}" runs as a plain-Node process, where ` +
          `require("electron") throws MODULE_NOT_FOUND and kills it at startup (the ` +
          `v1.4.129-rc.1 daemon outage). Keep electron imports out of its module graph.`
      )
    }
  }
}

export function createPlainNodeEntryGuardPlugin(): Plugin {
  return {
    name: 'orca-plain-node-entry-guard',
    writeBundle(_options: NormalizedOutputOptions, bundle: OutputBundle) {
      // Why: skip in `electron-vite dev` watch mode — the smoke would respawn on
      // every rebuild, and the guard only needs to gate produced builds.
      if (this.meta.watchMode) {
        return
      }
      const chunks = Object.values(bundle).filter(
        (item): item is OutputChunk => item.type === 'chunk'
      )
      const byFileName = new Map(chunks.map((chunk) => [chunk.fileName, chunk]))
      const entryByName = new Map<string, OutputChunk>()
      for (const chunk of chunks) {
        if (chunk.isEntry && chunk.name) {
          entryByName.set(chunk.name, chunk)
        }
      }

      // Why: a renamed/removed plain-Node input key would make the electron-require
      // check skip that entry silently, re-enabling the v1.4.129-rc.1 daemon outage;
      // hard-fail so a missing guarded entry breaks the build instead of passing vacuously.
      const unresolved = PLAIN_NODE_ENTRY_NAMES.filter((name) => !entryByName.has(name))
      if (unresolved.length > 0) {
        throw new Error(
          `[plain-node-entry-guard] no emitted entry chunk for ${unresolved
            .map((name) => `"${name}"`)
            .join(', ')}. A plain-Node entry was renamed or removed without updating ` +
            `PLAIN_NODE_ENTRY_NAMES, which would silently disable the electron-require guard ` +
            `(the v1.4.129-rc.1 daemon outage). Update PLAIN_NODE_ENTRY_NAMES to match the ` +
            `build's entry names.`
        )
      }

      for (const entryName of PLAIN_NODE_ENTRY_NAMES) {
        const entry = entryByName.get(entryName) as OutputChunk
        assertNoElectronRequire(entryName, entry, byFileName)
      }
    }
  }
}
