#!/usr/bin/env node

// Vendors the aterm terminal-engine SOURCE into rust/aterm so orca-terminal
// builds against an in-tree copy — clean-machine and packaged builds need no
// external ~/aterm checkout. Run this to refresh the engine to the latest aterm
// ("get the latest aterm"): point it at a live aterm checkout and re-vendor.
//
// Usage: node config/scripts/vendor-aterm.mjs [--src <path>]
//   default src: $ATERM_SRC or ~/aterm

import { cpSync, rmSync, mkdirSync, existsSync, writeFileSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import { dirname, join, resolve, basename } from 'node:path'
import { fileURLToPath } from 'node:url'
import { homedir } from 'node:os'

const scriptPath = fileURLToPath(import.meta.url)
const projectDir = resolve(dirname(scriptPath), '../..')
const destDir = resolve(projectDir, 'rust/aterm')

function srcArg() {
  const i = process.argv.indexOf('--src')
  if (i >= 0 && process.argv[i + 1]) {
    return resolve(process.argv[i + 1])
  }
  return resolve(process.env.ATERM_SRC || join(homedir(), 'aterm'))
}

const srcDir = srcArg()
if (!existsSync(join(srcDir, 'Cargo.toml')) || !existsSync(join(srcDir, 'crates'))) {
  console.error(`[vendor-aterm] no aterm workspace at ${srcDir} (pass --src <path> or set ATERM_SRC)`)
  process.exit(1)
}

// Only the build inputs: the crate sources + workspace manifests + license. The
// non-build top-level trees (docs/apps/tools/packaging/scripts) are simply not
// copied (we only copy crates/ + TOP_FILES). Inside crates/ skip ONLY build
// artifacts and VCS — crate `src/` subdirs (e.g. shell-integration scripts that
// are include_str!'d) must be preserved.
const SKIP_DIRS = new Set(['target', '.git'])
const TOP_FILES = [
  'Cargo.toml',
  'Cargo.lock',
  'rust-toolchain.toml',
  'rustfmt.toml',
  'deny.toml',
  'LICENSE',
  'NOTICE'
]

console.log(`[vendor-aterm] vendoring ${srcDir} -> ${destDir}`)
rmSync(destDir, { recursive: true, force: true })
mkdirSync(destDir, { recursive: true })

// crates/ (recursive, skipping any nested target/.git)
cpSync(join(srcDir, 'crates'), join(destDir, 'crates'), {
  recursive: true,
  filter: (s) => !SKIP_DIRS.has(basename(s))
})
// vendor/ (recursive): the workspace root Cargo.toml `[patch.crates-io]` points
// winit at `vendor/winit`, so the dir must exist for cargo to RESOLVE the
// workspace. It's only a patch entry (the GUI's windowing dep) and is not in the
// wasm dep tree, so winit is never compiled for the wasm engine build.
if (existsSync(join(srcDir, 'vendor'))) {
  cpSync(join(srcDir, 'vendor'), join(destDir, 'vendor'), {
    recursive: true,
    filter: (s) => !SKIP_DIRS.has(basename(s))
  })
}
for (const f of TOP_FILES) {
  if (existsSync(join(srcDir, f))) {
    cpSync(join(srcDir, f), join(destDir, f))
  }
}

// Record the source revision for provenance (best-effort; not all checkouts are git).
const rev = spawnSync('git', ['-C', srcDir, 'rev-parse', 'HEAD'], { encoding: 'utf8' })
const revText = rev.status === 0 ? rev.stdout.trim() : 'unknown'
writeFileSync(join(destDir, 'VENDORED_REV.txt'), `${revText}\n`)

console.log(`[vendor-aterm] done. aterm rev ${revText}`)
console.log('[vendor-aterm] rebuild the addon with: pnpm run build:terminal-addon --force')
