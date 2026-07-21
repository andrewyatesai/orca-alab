import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const DEFAULT_GIT_WASM_GLUE = resolve(import.meta.dirname, '../../src/relay/wasm/orca_git_wasm.js')
const GENERATED_ASYNC_FALLBACK =
  "    module_or_path = new URL('orca_git_wasm_bg.wasm', import.meta.url)"
const RELAY_SYNC_ONLY_FAILURE =
  "    throw new Error('The relay wasm must be initialized with initSync and embedded bytes')"

export function rewriteRelaySyncOnlyWasmGlue(source) {
  const firstMatch = source.indexOf(GENERATED_ASYNC_FALLBACK)
  if (firstMatch === -1 || source.includes(GENERATED_ASYNC_FALLBACK, firstMatch + 1)) {
    throw new Error('Expected exactly one wasm-bindgen async fallback in the relay git wasm glue')
  }
  return source.replace(GENERATED_ASYNC_FALLBACK, RELAY_SYNC_ONLY_FAILURE)
}

/**
 * The relay ships embedded wasm bytes and initializes them synchronously. Its
 * generated async file fallback cannot work because no sibling wasm is shipped.
 */
export function relaySyncOnlyWasmGluePlugin(gitWasmGlue = DEFAULT_GIT_WASM_GLUE) {
  const expectedGluePath = resolve(gitWasmGlue)
  return {
    name: 'orca-relay-sync-only-wasm-glue',
    setup(buildContext) {
      buildContext.onLoad({ filter: /[/\\]orca_git_wasm\.js$/ }, (args) => {
        if (resolve(args.path) !== expectedGluePath) {
          return undefined
        }
        return {
          contents: rewriteRelaySyncOnlyWasmGlue(readFileSync(args.path, 'utf8')),
          loader: 'js'
        }
      })
    }
  }
}
