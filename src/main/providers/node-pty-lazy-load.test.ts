import { afterEach, describe, expect, it, vi } from 'vitest'
import type * as NodePty from 'node-pty'
import { _setNodePtyImportForTest, loadNodePty, nodePtyLoadFailure } from './node-pty-lazy-load'

describe('loadNodePty', () => {
  afterEach(() => {
    _setNodePtyImportForTest(null)
  })

  it('returns the module on success and caches it', async () => {
    const spawn = vi.fn()
    const importer = vi.fn(async () => ({ spawn }) as unknown as typeof NodePty)
    _setNodePtyImportForTest(importer)

    const first = await loadNodePty()
    expect(first.spawn).toBe(spawn)
    expect(await loadNodePty()).toBe(first)
    expect(importer).toHaveBeenCalledTimes(1)
    expect(nodePtyLoadFailure()).toBeUndefined()
  })

  it('rejects with the native cause when the binding fails to load, and remembers it', async () => {
    const importer = vi.fn(async () => {
      throw new Error('The module pty.node was compiled against NODE_MODULE_VERSION 118')
    })
    _setNodePtyImportForTest(importer)

    await expect(loadNodePty()).rejects.toThrow(/NODE_MODULE_VERSION 118/)
    // Why: a broken binding cannot heal in-process; later spawns must fail fast with the same cause.
    await expect(loadNodePty()).rejects.toThrow(/NODE_MODULE_VERSION 118/)
    expect(importer).toHaveBeenCalledTimes(1)
    expect(nodePtyLoadFailure()).toContain('NODE_MODULE_VERSION 118')
    expect(nodePtyLoadFailure()).toContain('node-pty failed to load')
  })
})
