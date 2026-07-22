#!/usr/bin/env node

// Runnable gate for the TS↔Rust differential parity suite. Two legs:
//   1. Regenerate rust_outputs.json by running the orca-parity binary over the
//      shared vector corpus (the Rust leg — also golden-checks each case).
//   2. Run the vitest driver (tools/parity/parity.test.ts) which asserts
//      TS == Rust (and TS == golden) for every case.
//
// Toolchain: the orca-crates workspace needs rustc 1.96, but the machine default
// `cargo` is a Homebrew 1.95 that shadows rustup and whose child rustc is also
// the shadow. We pin BOTH the cargo and rustc to the rustup `stable` toolchain,
// matching config/scripts/build-aterm-wasm.mjs.
//
// Fully offline: the workspace resolves against rust/vendor (which carries the
// complete lockfile closure, web-time included). A prebuilt binary is only a
// fallback when rustup is unavailable; preferring it can silently run stale code.

import { spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, resolve } from 'node:path'
import { orcaParityExecutablePaths } from './rust-host-executable-paths.mjs'

const projectDir = resolve(import.meta.dirname, '../..')
const require = createRequire(import.meta.url)
const vectorsDir = resolve(projectDir, 'tools/parity/vectors')
const outputsFile = resolve(projectDir, 'tools/parity/rust_outputs.json')
const vitestCli = resolve(dirname(require.resolve('vitest/package.json')), 'vitest.mjs')

function rustupBin(tool) {
  const r = spawnSync('rustup', ['which', tool, '--toolchain', 'stable'], { encoding: 'utf8' })
  return r.status === 0 ? r.stdout.trim() : null
}

function run(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, { stdio: 'inherit', cwd: projectDir, ...opts })
  if (r.error) {
    console.error(`[parity] failed to start \`${cmd}\`: ${r.error.message}`)
    process.exit(1)
  }
  return r.status ?? 1
}

// Leg 1: regenerate the Rust outputs.
const prebuilt = orcaParityExecutablePaths(projectDir).find((path) => existsSync(path))

let rustStatus
const cargoBin = rustupBin('cargo')
const rustcBin = rustupBin('rustc')
if (cargoBin && rustcBin) {
  console.log('[parity] building + running orca-parity (rustup stable, offline via rust/vendor)')
  rustStatus = run(
    cargoBin,
    [
      'run',
      '--quiet',
      '-p',
      'orca-parity',
      '--manifest-path',
      'rust/Cargo.toml',
      '--',
      vectorsDir,
      outputsFile
    ],
    { env: { ...process.env, RUSTC: rustcBin } }
  )
} else if (prebuilt) {
  console.warn(`[parity] rustup stable unavailable; using prebuilt ${prebuilt}`)
  rustStatus = run(prebuilt, [vectorsDir, outputsFile])
} else {
  console.error('[parity] no prebuilt orca-parity and rustup stable is unavailable')
  process.exit(1)
}

if (rustStatus !== 0) {
  console.error(`[parity] Rust leg failed (exit ${rustStatus})`)
  process.exit(rustStatus)
}

// Leg 2: the TS differential driver.
const vitestStatus = run(process.execPath, [
  vitestCli,
  'run',
  '--config',
  'config/vitest.parity.config.ts'
])
process.exit(vitestStatus)
