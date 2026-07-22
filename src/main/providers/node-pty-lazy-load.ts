import type * as NodePty from 'node-pty'

// Why: node-pty require()s its native pty.node at import, so a static import from main-process
// startup turns a broken binding (ABI mismatch, corrupted install) into a pre-whenReady crash
// with no window; loading per spawn surfaces the failure as a spawn error instead.

let cached: typeof NodePty | undefined
let failureDetail: string | undefined

// Why: test seam — vitest wraps throwing module-mock factories in its own error, hiding the native cause.
let importNodePty: () => Promise<typeof NodePty> = () => import('node-pty')

export function _setNodePtyImportForTest(importer: (() => Promise<typeof NodePty>) | null): void {
  importNodePty = importer ?? (() => import('node-pty'))
  cached = undefined
  failureDetail = undefined
}

/** Why the last `loadNodePty()` rejected, or undefined until a load has failed. */
export function nodePtyLoadFailure(): string | undefined {
  return failureDetail
}

/** Load node-pty on first use. Once broken, every call rejects with the remembered cause. */
export async function loadNodePty(): Promise<typeof NodePty> {
  if (cached) {
    return cached
  }
  if (failureDetail !== undefined) {
    throw new Error(failureDetail)
  }
  try {
    cached = await importNodePty()
    return cached
  } catch (error) {
    // Why: keep the real cause (e.g. a NODE_MODULE_VERSION mismatch) so the spawn error names it.
    failureDetail = `node-pty failed to load — terminals cannot start (reinstall Orca or rebuild native modules): ${error instanceof Error ? error.message : String(error)}`
    throw new Error(failureDetail)
  }
}
