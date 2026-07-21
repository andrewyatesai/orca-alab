import { describe, expect, it } from 'vitest'
import type { Plugin } from 'vite'
import {
  createRendererChunkBudgetPlugin,
  createRendererWorkerChunkBudgetPlugin
} from '../../../build-plugins/renderer-chunk-budget'

const MEBIBYTE = 1024 * 1024

function syntheticChunk(
  fileName: string,
  mebibytes: number,
  options: { imports?: string[]; isEntry?: boolean } = {}
) {
  return {
    type: 'chunk' as const,
    fileName,
    code: 'x'.repeat(Math.round(mebibytes * MEBIBYTE)),
    imports: options.imports ?? [],
    isEntry: options.isEntry ?? false
  }
}

function runGenerateBundle(plugin: Plugin, bundle: Record<string, unknown>): void {
  const hook = plugin.generateBundle
  if (!hook) {
    throw new Error('Expected a generateBundle hook')
  }
  const handler = typeof hook === 'function' ? hook : hook.handler
  const context = {
    error(message: string | { message: string }): never {
      throw new Error(typeof message === 'string' ? message : message.message)
    }
  }
  handler.call(context as never, {} as never, bundle as never, false)
}

describe('renderer chunk budget plugin', () => {
  it('accepts a web entry whose complete static closure is below budget', () => {
    const bundle = {
      'entry.js': syntheticChunk('entry.js', 1.5, {
        isEntry: true,
        imports: ['shared.js']
      }),
      'shared.js': syntheticChunk('shared.js', 0.4)
    }

    expect(() => runGenerateBundle(createRendererChunkBudgetPlugin('web'), bundle)).not.toThrow()
  })

  it('rejects split eager desktop chunks that individually pass the per-file cap', () => {
    const bundle = {
      'entry.js': syntheticChunk('entry.js', 1.5, {
        isEntry: true,
        imports: ['shared-a.js', 'shared-b.js']
      }),
      'shared-a.js': syntheticChunk('shared-a.js', 1.5),
      'shared-b.js': syntheticChunk('shared-b.js', 1.4)
    }

    expect(() => runGenerateBundle(createRendererChunkBudgetPlugin('desktop'), bundle)).toThrow(
      /entry entry\.js statically reaches 3 chunks totaling 4\.40 MiB;.*4\.25 MiB/
    )
  })

  it('retains the lazy per-file cap', () => {
    const bundle = {
      'entry.js': syntheticChunk('entry.js', 0.1, { isEntry: true }),
      'lazy.js': syntheticChunk('lazy.js', 4.8)
    }

    expect(() => runGenerateBundle(createRendererChunkBudgetPlugin('desktop'), bundle)).toThrow(
      /lazy chunk lazy\.js is 4\.80 MiB;.*4\.75 MiB/
    )
  })

  it('runs a separate worker policy and rejects a split worker above its closure cap', () => {
    const bundle = {
      'worker.js': syntheticChunk('worker.js', 4, {
        isEntry: true,
        imports: ['worker-shared.js']
      }),
      'worker-shared.js': syntheticChunk('worker-shared.js', 3.5)
    }

    expect(() => runGenerateBundle(createRendererWorkerChunkBudgetPlugin(), bundle)).toThrow(
      /worker entry worker\.js statically reaches 2 chunks totaling 7\.50 MiB;.*7\.25 MiB/
    )
  })
})
