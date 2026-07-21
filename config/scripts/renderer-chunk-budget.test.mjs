import { describe, expect, it } from 'vitest'
import {
  createRendererChunkBudgetPlugin,
  createRendererWorkerChunkBudgetPlugin
} from '../../build-plugins/renderer-chunk-budget'

const MEBIBYTE = 1024 * 1024

function chunk(fileName, mebibytes, { imports = [], isEntry = false } = {}) {
  return {
    type: 'chunk',
    code: 'x'.repeat(Math.ceil(mebibytes * MEBIBYTE)),
    fileName,
    imports,
    isEntry
  }
}

function runBudget(plugin, chunks) {
  const hook =
    typeof plugin.generateBundle === 'function'
      ? plugin.generateBundle
      : plugin.generateBundle?.handler
  if (!hook) {
    throw new Error('Chunk budget plugin has no generateBundle hook')
  }
  const bundle = Object.fromEntries(chunks.map((candidate) => [candidate.fileName, candidate]))
  return () =>
    hook.call(
      {
        error: (message) => {
          throw new Error(message)
        }
      },
      {},
      bundle,
      false
    )
}

describe('renderer chunk budgets', () => {
  it('rejects an eager desktop entry split across individually valid chunks', () => {
    const run = runBudget(createRendererChunkBudgetPlugin('desktop'), [
      chunk('index.js', 2.2, { imports: ['shared.js'], isEntry: true }),
      chunk('shared.js', 2.2)
    ])

    expect(run).toThrow(/desktop renderer entry index\.js.+total eager-entry budget/u)
  })

  it('does not count a dynamic lazy chunk in the eager static closure', () => {
    const entry = chunk('web.js', 1.75, { isEntry: true })
    entry.dynamicImports = ['editor.js']
    const run = runBudget(createRendererChunkBudgetPlugin('web'), [entry, chunk('editor.js', 4.5)])

    expect(run).not.toThrow()
  })

  it('rejects a worker split that exceeds its total closure budget', () => {
    const run = runBudget(createRendererWorkerChunkBudgetPlugin(), [
      chunk('ts.worker.js', 7, { imports: ['worker-shared.js'], isEntry: true }),
      chunk('worker-shared.js', 0.5)
    ])

    expect(run).toThrow(/renderer worker entry ts\.worker\.js.+total eager-entry budget/u)
  })

  it('still rejects an oversized individual lazy chunk', () => {
    const run = runBudget(createRendererChunkBudgetPlugin('desktop'), [
      chunk('index.js', 1, { isEntry: true }),
      chunk('editor.js', 4.8)
    ])

    expect(run).toThrow(/lazy chunk editor\.js.+per-file lazy budget/u)
  })
})
