import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

const loaderCases = [
  {
    file: 'crypto-wasm/browser-crypto-wasm.ts',
    calls: ['initCryptoWasm({ module_or_path: wasmUrl })']
  },
  {
    file: 'git-wasm/git-line-stats.ts',
    calls: ['initGitWasm({ module_or_path: wasmUrl })']
  },
  {
    file: 'pane-manager/aterm/load-aterm-gpu.ts',
    calls: ['init({ module_or_path: wasmUrl })']
  },
  {
    file: 'pane-manager/aterm/load-aterm.ts',
    calls: ['init({ module_or_path: wasmUrl })']
  },
  {
    file: 'pane-manager/aterm/aterm-worker-engine-build.ts',
    calls: ['init({ module_or_path: wasmUrl })', 'gpuInit({ module_or_path: gpuWasmUrl })']
  }
] as const

describe('wasm-bindgen async loader signatures', () => {
  for (const { file, calls } of loaderCases) {
    it(`passes a single options object in ${file}`, () => {
      const source = readFileSync(resolve(import.meta.dirname, file), 'utf8')

      for (const call of calls) {
        expect(source).toContain(call)
      }
      expect(source).not.toMatch(
        /(?:\binit(?:CryptoWasm|GitWasm)?|\bgpuInit)\(\s*(?:gpuWasmUrl|wasmUrl)\s*\)/u
      )
    })
  }
})
