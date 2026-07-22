import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import {
  cachedWasmBindgenExecutablePath,
  orcaParityExecutablePaths
} from './rust-host-executable-paths.mjs'

describe('Rust host executable paths', () => {
  it('uses Windows executable names for cached Rust host tools', () => {
    const projectDir = join('C:', 'orca')

    expect(cachedWasmBindgenExecutablePath(join(projectDir, 'tooling'), 'win32')).toBe(
      join(projectDir, 'tooling', 'bin', 'wasm-bindgen.exe')
    )
    expect(orcaParityExecutablePaths(projectDir, 'win32')).toEqual([
      join(projectDir, 'rust', 'target', 'debug', 'orca-parity.exe'),
      join(projectDir, 'rust', 'target', 'release', 'orca-parity.exe')
    ])
  })

  it.each(['darwin', 'linux'])('preserves extensionless executable names on %s', (platform) => {
    const projectDir = join('workspace', 'orca')

    expect(cachedWasmBindgenExecutablePath(join(projectDir, 'tooling'), platform)).toBe(
      join(projectDir, 'tooling', 'bin', 'wasm-bindgen')
    )
    expect(orcaParityExecutablePaths(projectDir, platform)).toEqual([
      join(projectDir, 'rust', 'target', 'debug', 'orca-parity'),
      join(projectDir, 'rust', 'target', 'release', 'orca-parity')
    ])
  })
})
