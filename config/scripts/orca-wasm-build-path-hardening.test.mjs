import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

// Finding: the crypto wasm (embedded in the desktop app) and git wasm (embedded in
// the relay bundle uploaded to every remote host) were built WITHOUT the
// --remap-path-prefix hardening + embedded-path assertion that aterm uses, so a
// release build could leak the builder's home/username via panic/source strings.
// These build scripts require rust + wasm-bindgen + network, so we can't execute
// them here; assert at the source level that both defenses are wired in.
const ROOT = resolve(import.meta.dirname, '../..')

const SCRIPTS = [
  'config/scripts/build-orca-crypto-wasm.mjs',
  'config/scripts/build-orca-git-wasm.mjs'
]

describe('orca wasm build path hardening', () => {
  for (const file of SCRIPTS) {
    const source = readFileSync(resolve(ROOT, file), 'utf8')

    it(`${file} imports the remap + assertion helpers`, () => {
      expect(source).toMatch(
        /import \{[^}]*wasmCratePathRemapRustflags[^}]*\} from '\.\/wasm-build-paths\.mjs'/
      )
      expect(source).toContain('assertNoEmbeddedLocalBuildPaths')
    })

    it(`${file} injects the remap RUSTFLAGS into the wasm cargo build`, () => {
      expect(source).toContain('CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS')
      expect(source).toMatch(/wasmCratePathRemapRustflags\(\{\s*root: ROOT,\s*crateSource:/)
    })

    it(`${file} asserts the optimized wasm embeds no local build path`, () => {
      // The optimized _bg.wasm (`bg`) is the single source for every copy/embed.
      expect(source).toMatch(
        /assertNoEmbeddedLocalBuildPaths\(readFileSync\(bg\),[\s\S]*?STEM.*_bg\.wasm/
      )
    })
  }
})
