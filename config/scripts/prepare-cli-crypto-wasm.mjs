#!/usr/bin/env node

import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import { transformSync } from 'esbuild'

/**
 * Converts wasm-bindgen's browser-oriented ESM glue into the CommonJS module
 * shape emitted by the TypeScript CLI build.
 */
export function prepareCliCryptoWasm({
  projectDir = path.resolve(import.meta.dirname, '..', '..')
} = {}) {
  const sourcePath = path.join(projectDir, 'src', 'shared', 'crypto-wasm', 'orca_crypto_wasm.js')
  const outputPath = path.join(projectDir, 'out', 'shared', 'crypto-wasm', 'orca_crypto_wasm.js')
  const source = readFileSync(sourcePath, 'utf8')
  const transformed = transformSync(source, {
    banner: 'const __orcaModuleUrl = require("node:url").pathToFileURL(__filename).href;',
    define: {
      'import.meta.url': '__orcaModuleUrl'
    },
    format: 'cjs',
    legalComments: 'none',
    platform: 'node',
    sourcefile: sourcePath,
    target: 'node24'
  })

  mkdirSync(path.dirname(outputPath), { recursive: true })
  writeFileSync(outputPath, transformed.code, 'utf8')
  return outputPath
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const outputPath = prepareCliCryptoWasm()
  console.log(`[cli-crypto-wasm] wrote ${path.relative(process.cwd(), outputPath)}`)
}
