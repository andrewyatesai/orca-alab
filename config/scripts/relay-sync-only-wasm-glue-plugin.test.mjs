import { describe, expect, it } from 'vitest'
import {
  relaySyncOnlyWasmGluePlugin,
  rewriteRelaySyncOnlyWasmGlue
} from './relay-sync-only-wasm-glue-plugin.mjs'

const generatedFallback = "    module_or_path = new URL('orca_git_wasm_bg.wasm', import.meta.url)"

describe('relay sync-only wasm glue', () => {
  it('replaces the generated async file fallback with a clear sync-only failure', () => {
    const rewritten = rewriteRelaySyncOnlyWasmGlue(`before\n${generatedFallback}\nafter`)

    expect(rewritten).not.toContain('import.meta')
    expect(rewritten).toContain(
      "throw new Error('The relay wasm must be initialized with initSync and embedded bytes')"
    )
  })

  it.each(['no generated fallback', `${generatedFallback}\n${generatedFallback}`])(
    'rejects generator drift: %s',
    (source) => {
      expect(() => rewriteRelaySyncOnlyWasmGlue(source)).toThrow(
        'Expected exactly one wasm-bindgen async fallback'
      )
    }
  )

  it('exposes the concrete esbuild plugin used by relay bundle callers', () => {
    expect(relaySyncOnlyWasmGluePlugin()).toMatchObject({
      name: 'orca-relay-sync-only-wasm-glue',
      setup: expect.any(Function)
    })
  })
})
