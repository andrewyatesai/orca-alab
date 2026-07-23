import { describe, expect, it } from 'vitest'
import type { Plugin } from 'vite'
import { createPlainNodeEntryGuardPlugin } from '../../../build-plugins/plain-node-entry-guard'

// The three plain-Node fork entries the guard must always find in a produced build.
const PLAIN_NODE_ENTRY_NAMES = [
  'parcel-watcher-process-entry',
  'computer-sidecar',
  'agent-hooks/managed-agent-hook-controls'
] as const

function entryChunk(
  name: string,
  code: string,
  options: { imports?: string[]; dynamicImports?: string[] } = {}
) {
  return {
    type: 'chunk' as const,
    name,
    fileName: `${name}.js`,
    isEntry: true,
    code,
    imports: options.imports ?? [],
    dynamicImports: options.dynamicImports ?? []
  }
}

function sharedChunk(fileName: string, code: string, imports: string[] = []) {
  return {
    type: 'chunk' as const,
    name: undefined,
    fileName,
    isEntry: false,
    code,
    imports,
    dynamicImports: []
  }
}

function runWriteBundle(plugin: Plugin, bundle: Record<string, unknown>, watchMode = false): void {
  const hook = plugin.writeBundle
  if (!hook) {
    throw new Error('Expected a writeBundle hook')
  }
  const handler = typeof hook === 'function' ? hook : hook.handler
  const context = { meta: { watchMode } }
  handler.call(context as never, {} as never, bundle as never)
}

function bundleWithAllEntries(overrides: Record<string, unknown> = {}) {
  const bundle: Record<string, unknown> = {}
  for (const name of PLAIN_NODE_ENTRY_NAMES) {
    bundle[`${name}.js`] = entryChunk(name, 'const x = 1')
  }
  return { ...bundle, ...overrides }
}

describe('plain-node entry guard plugin', () => {
  it('passes when every plain-Node entry is present and none reaches require("electron")', () => {
    expect(() =>
      runWriteBundle(createPlainNodeEntryGuardPlugin(), bundleWithAllEntries())
    ).not.toThrow()
  })

  it('fails the build when a guarded plain-Node entry is renamed or removed', () => {
    const bundle = bundleWithAllEntries()
    delete bundle['computer-sidecar.js']

    expect(() => runWriteBundle(createPlainNodeEntryGuardPlugin(), bundle)).toThrow(
      /no emitted entry chunk for "computer-sidecar"/
    )
  })

  it('lists every unresolved entry when multiple are missing', () => {
    const bundle = bundleWithAllEntries()
    delete bundle['computer-sidecar.js']
    delete bundle['parcel-watcher-process-entry.js']

    expect(() => runWriteBundle(createPlainNodeEntryGuardPlugin(), bundle)).toThrow(
      /"parcel-watcher-process-entry".*"computer-sidecar"/s
    )
  })

  it('still catches an electron require reachable from a present entry', () => {
    const bundle = bundleWithAllEntries({
      'computer-sidecar.js': entryChunk('computer-sidecar', 'const x = 1', {
        imports: ['shared.js']
      }),
      'shared.js': sharedChunk('shared.js', 'const e = require("electron")')
    })

    expect(() => runWriteBundle(createPlainNodeEntryGuardPlugin(), bundle)).toThrow(
      /reaches chunk "shared\.js" that requires electron/
    )
  })

  it('skips the guard in watch mode even when an entry is missing', () => {
    const bundle = bundleWithAllEntries()
    delete bundle['computer-sidecar.js']

    expect(() => runWriteBundle(createPlainNodeEntryGuardPlugin(), bundle, true)).not.toThrow()
  })
})
