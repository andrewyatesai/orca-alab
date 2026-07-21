import type { Plugin } from 'vite'

const MEBIBYTE = 1024 * 1024

type ChunkBudgetPolicy = {
  label: string
  maxEagerChunkBytes: number
  maxLazyChunkBytes: number
  maxEagerEntryBytes: number
}

type BudgetChunk = {
  code: string
  fileName: string
  imports: string[]
  isEntry: boolean
}

export const RENDERER_CHUNK_BUDGETS = {
  desktop: {
    label: 'desktop renderer',
    // Baseline: index's largest file is 1.98 MiB; its full static closure is
    // 3.84 MiB. The 2.25/4.25 MiB ratchets leave 13%/11% headroom.
    maxEagerChunkBytes: 2.25 * MEBIBYTE,
    maxLazyChunkBytes: 4.75 * MEBIBYTE,
    maxEagerEntryBytes: 4.25 * MEBIBYTE
  },
  web: {
    label: 'web renderer',
    // Baseline: the pre-connect web entry and its static closure are 1.77 MiB.
    // A 2 MiB ratchet leaves 13% headroom; post-connect App remains lazy.
    maxEagerChunkBytes: 2 * MEBIBYTE,
    maxLazyChunkBytes: 4.75 * MEBIBYTE,
    maxEagerEntryBytes: 2 * MEBIBYTE
  },
  worker: {
    label: 'renderer worker',
    // Baseline: Monaco's TypeScript worker is a single 6.69 MiB entry. Apply
    // the same 7.25 MiB cap to each file and its full static closure so splitting
    // that worker cannot evade the ratchet. Smaller workers share the cap.
    maxEagerChunkBytes: 7.25 * MEBIBYTE,
    maxLazyChunkBytes: 7.25 * MEBIBYTE,
    maxEagerEntryBytes: 7.25 * MEBIBYTE
  }
} as const satisfies Record<string, ChunkBudgetPolicy>

function formatMebibytes(bytes: number): string {
  return `${(bytes / MEBIBYTE).toFixed(2)} MiB`
}

function collectStaticClosure(
  entryFileName: string,
  chunksByFileName: ReadonlyMap<string, BudgetChunk>
): Set<string> {
  const reachableFiles = new Set<string>()
  const visit = (fileName: string): void => {
    if (reachableFiles.has(fileName)) {
      return
    }
    const chunk = chunksByFileName.get(fileName)
    if (!chunk) {
      return
    }
    reachableFiles.add(fileName)
    for (const importedFile of chunk.imports) {
      visit(importedFile)
    }
  }
  visit(entryFileName)
  return reachableFiles
}

function createChunkBudgetPlugin(policy: ChunkBudgetPolicy): Plugin {
  return {
    name: `orca-${policy.label.replaceAll(' ', '-')}-chunk-budget`,
    generateBundle(_outputOptions, bundle) {
      const chunks: BudgetChunk[] = Object.values(bundle)
        .filter((output) => output.type === 'chunk')
        .map((chunk) => ({
          code: chunk.code,
          fileName: chunk.fileName,
          imports: chunk.imports,
          isEntry: chunk.isEntry
        }))
      const chunksByFileName = new Map(chunks.map((chunk) => [chunk.fileName, chunk]))
      const eagerClosures = chunks
        .filter((chunk) => chunk.isEntry)
        .map((entry) => ({
          entry,
          files: collectStaticClosure(entry.fileName, chunksByFileName)
        }))
      const eagerFiles = new Set(eagerClosures.flatMap(({ files }) => [...files]))

      for (const chunk of chunks) {
        const bytes = Buffer.byteLength(chunk.code)
        const eager = eagerFiles.has(chunk.fileName)
        const limit = eager ? policy.maxEagerChunkBytes : policy.maxLazyChunkBytes
        if (bytes > limit) {
          const kind = eager ? 'eager' : 'lazy'
          this.error(
            `${policy.label} ${kind} chunk ${chunk.fileName} is ${formatMebibytes(bytes)}; ` +
              `the per-file ${kind} budget is ${formatMebibytes(limit)}`
          )
        }
      }

      for (const { entry, files } of eagerClosures) {
        const bytes = [...files].reduce(
          (total, fileName) =>
            total + Buffer.byteLength(chunksByFileName.get(fileName)?.code ?? ''),
          0
        )
        if (bytes > policy.maxEagerEntryBytes) {
          const budget = formatMebibytes(policy.maxEagerEntryBytes)
          this.error(
            `${policy.label} entry ${entry.fileName} statically reaches ${files.size} chunks ` +
              `totaling ${formatMebibytes(bytes)}; the total eager-entry budget is ${budget}`
          )
        }
      }
    }
  }
}

/**
 * Vite's generic 500 kB warning cannot distinguish startup code from Orca's
 * intentional, lazy Monaco + TipTap + Mermaid editor bundle. Enforce separate
 * per-file eager/lazy ratchets plus the entire static closure of every entry.
 */
export function createRendererChunkBudgetPlugin(target: 'desktop' | 'web'): Plugin {
  return createChunkBudgetPlugin(RENDERER_CHUNK_BUDGETS[target])
}

/** Worker child builds do not run top-level Vite plugins, so each worker gets
 * its own per-file and full-static-closure budget plugin. */
export function createRendererWorkerChunkBudgetPlugin(): Plugin {
  return createChunkBudgetPlugin(RENDERER_CHUNK_BUDGETS.worker)
}
