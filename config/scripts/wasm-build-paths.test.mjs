import { describe, expect, it } from 'vitest'
import {
  assertNoEmbeddedLocalBuildPaths,
  containsLocalCargoSourcePath,
  localWasmBuildPaths,
  wasmPathRemapRustflags
} from './wasm-build-paths.mjs'

const fixture = {
  root: '/Users/example/work/orc',
  atermSource: '/private/tmp/orca-aterm-wasm-123/aterm',
  env: {},
  home: '/Users/example'
}

describe('WASM build path portability', () => {
  it('remaps machine-specific roots to stable virtual paths', () => {
    expect(wasmPathRemapRustflags(fixture)).toEqual([
      '--remap-path-prefix=/private/tmp/orca-aterm-wasm-123/aterm=/aterm',
      '--remap-path-prefix=/Users/example/work/orc=/orca',
      '--remap-path-prefix=/Users/example/.cargo=/cargo',
      '--remap-path-prefix=/Users/example=/builder-home'
    ])
    expect(localWasmBuildPaths(fixture)).toEqual([
      '/private/tmp/orca-aterm-wasm-123/aterm',
      '/Users/example/work/orc',
      '/Users/example/.cargo',
      '/Users/example'
    ])
  })

  it('rejects an embedded local build root', () => {
    expect(() =>
      assertNoEmbeddedLocalBuildPaths(
        Buffer.from('panic at /Users/example/.cargo/registry/src/crate/src/lib.rs'),
        { ...fixture, label: 'cpu.wasm' }
      )
    ).toThrow(/cpu\.wasm embeds a local build path/)
    expect(() =>
      assertNoEmbeddedLocalBuildPaths(
        Buffer.from('panic at /cargo/registry/src/crate/src/lib.rs'),
        fixture
      )
    ).not.toThrow()
  })

  it('recognizes common Cargo source leaks offline', () => {
    expect(
      containsLocalCargoSourcePath('/Users/alice/.cargo/registry/src/index/crate/src/lib.rs')
    ).toBe(true)
    expect(containsLocalCargoSourcePath('/home/alice/.cargo/git/checkouts/crate/src/lib.rs')).toBe(
      true
    )
    expect(
      containsLocalCargoSourcePath('C:\\Users\\alice\\.cargo\\registry\\src\\crate\\src\\lib.rs')
    ).toBe(true)
    expect(containsLocalCargoSourcePath('/cargo/registry/src/index/crate/src/lib.rs')).toBe(false)
  })
})
