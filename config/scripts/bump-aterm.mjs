#!/usr/bin/env node

// Bump the aterm terminal engine. aterm is a PINNED git submodule at rust/aterm
// (github.com/andrewyatesai/aterm) — the previous source-vendoring is gone. This
// updates the submodule to the latest origin/main (or a given --rev) and rebuilds
// the wasm bindings, native terminal addon, and Rust daemon so generated artifacts
// and both Cargo locks match the new pin. The offline dependency vendor (rust/vendor)
// is kept; if a bump pulls a new crate the build will say so and you re-vendor it.
//
// Usage: node config/scripts/bump-aterm.mjs [--rev <sha|ref>]
//   default: origin/main (latest)

import { spawnSync } from 'node:child_process'
import { resolve } from 'node:path'

const projectDir = resolve(import.meta.dirname, '../..')
const submodule = resolve(projectDir, 'rust/aterm')

function revArg() {
  const i = process.argv.indexOf('--rev')
  return i >= 0 && process.argv[i + 1] ? process.argv[i + 1] : null
}

function run(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, { stdio: 'inherit', cwd: projectDir, ...opts })
  if (r.status !== 0) {
    console.error(`[bump-aterm] \`${cmd} ${args.join(' ')}\` failed (exit ${r.status})`)
    process.exit(r.status ?? 1)
  }
}

// Make sure the submodule is checked out, then fetch + pin the target revision.
run('git', ['submodule', 'update', '--init', 'rust/aterm'])
run('git', ['-C', 'rust/aterm', 'fetch', 'origin'])
const target = revArg() ?? 'origin/main'
run('git', ['-C', 'rust/aterm', 'checkout', '--detach', target])

const rev =
  spawnSync('git', ['-C', submodule, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).stdout?.trim() ??
  'unknown'
console.log(`[bump-aterm] aterm submodule pinned to ${rev}`)

// Regenerate every consumer so committed artifacts and workspace locks move together.
run('node', ['config/scripts/build-aterm-wasm.mjs'])
run('node', ['config/scripts/build-terminal-addon.mjs', '--force'])
run('node', ['config/scripts/build-rust-daemon.mjs'])

console.log('[bump-aterm] done.')
console.log(
  // Stage the pin AND the full glue (binary + JS + .d.ts) so both ABI halves move
  // together; the JS/.d.ts are git-tracked via .gitignore negations but skipped by the
  // formatter (.prettierignore) so they stay byte-exact with wasm-bindgen output.
  '[bump-aterm] stage the pin + locks + regenerated wasm + glue:\n' +
    '  git add rust/aterm rust/Cargo.lock native/orca-node/Cargo.lock ' +
    'src/renderer/src/lib/pane-manager/aterm/aterm_wasm* ' +
    'src/renderer/src/lib/pane-manager/aterm/aterm_gpu_web*'
)
