#!/usr/bin/env node

// Bump the aterm terminal engine. aterm is a PINNED git submodule at rust/aterm
// (github.com/andrewyatesai/aterm) — the previous source-vendoring is gone. This
// updates the submodule to the latest origin/main (or a given --rev) and rebuilds
// the wasm bindings + the native terminal addon so the regenerated artifacts match
// the new pin. The offline dependency vendor (rust/vendor) is kept; if a bump pulls
// a new crate the build will say so and you re-vendor it (see docs).
//
// Usage: node config/scripts/bump-aterm.mjs [--rev <sha|ref>]
//   default: origin/main (latest)

import { spawnSync } from 'node:child_process'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const projectDir = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
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

// Regenerate the engine artifacts so the committed wasm + native addon match the pin.
run('node', ['config/scripts/build-aterm-wasm.mjs'])
run('node', ['config/scripts/build-terminal-addon.mjs', '--force'])

console.log('[bump-aterm] done.')
console.log(
  '[bump-aterm] stage the pin + regenerated wasm:\n' +
    '  git add rust/aterm src/renderer/src/lib/pane-manager/aterm/aterm_wasm_bg.wasm ' +
    'src/renderer/src/lib/pane-manager/aterm/aterm_gpu_web_bg.wasm'
)
