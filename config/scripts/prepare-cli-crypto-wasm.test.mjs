import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { createRequire } from 'node:module'
import { pathToFileURL } from 'node:url'
import { describe, expect, it } from 'vitest'
import { prepareCliCryptoWasm } from './prepare-cli-crypto-wasm.mjs'

describe('prepareCliCryptoWasm', () => {
  it('writes loadable CommonJS from wasm-bindgen ESM glue', () => {
    const projectDir = mkdtempSync(path.join(tmpdir(), 'orca-cli-wasm-'))
    const sourceDir = path.join(projectDir, 'src', 'shared', 'crypto-wasm')
    mkdirSync(sourceDir, { recursive: true })
    writeFileSync(
      path.join(sourceDir, 'orca_crypto_wasm.js'),
      'export const value = import.meta.url\n',
      'utf8'
    )

    const outputPath = prepareCliCryptoWasm({ projectDir })
    const loaded = createRequire(import.meta.url)(outputPath)

    expect(loaded.value).toBe(pathToFileURL(realpathSync(outputPath)).href)
    expect(readFileSync(outputPath, 'utf8')).toContain('module.exports')
    rmSync(projectDir, { recursive: true, force: true })
  })
})
