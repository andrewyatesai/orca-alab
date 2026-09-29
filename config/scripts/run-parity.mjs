#!/usr/bin/env node

// Runnable gate for the TS↔Rust differential parity suite. Two legs:
//   1. Regenerate rust_outputs.json by running the orca-parity binary over the
//      shared vector corpus (the Rust leg — also golden-checks each case).
//   2. Run the vitest driver (tools/parity/parity.test.ts) which asserts
//      TS == Rust (and TS == golden) for every case.
//
// Toolchain: `targo --unverified run` (the Trust toolchain's fast lane; see
// rust-toolchain-lane.mjs — no stock fallback on a host Trust serves).
//
// Fully offline: cargo runs inside rust/, so rust/.cargo/config.toml resolves the
// workspace against rust/vendor (which carries the complete lockfile closure,
// web-time included). A prebuilt binary is only a fallback when no Rust
// toolchain is available; preferring it can silently run stale code.

import { spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, resolve } from 'node:path'
import { orcaParityExecutablePaths } from './rust-host-executable-paths.mjs'
import { RustToolchainError, cargoInvocation } from './rust-toolchain-lane.mjs'

const projectDir = resolve(import.meta.dirname, '../..')
const require = createRequire(import.meta.url)
const vectorsDir = resolve(projectDir, 'tools/parity/vectors')
const outputsFile = resolve(projectDir, 'tools/parity/rust_outputs.json')
const vitestCli = resolve(dirname(require.resolve('vitest/package.json')), 'vitest.mjs')

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
let invocation = null
let toolchainError = null
try {
  invocation = cargoInvocation({
    verb: 'run',
    args: [
      '--quiet',
      '-p',
      'orca-parity',
      '--manifest-path',
      resolve(projectDir, 'rust/Cargo.toml'),
      '--offline',
      '--',
      vectorsDir,
      outputsFile
    ],
    label: 'parity'
  })
} catch (error) {
  if (!(error instanceof RustToolchainError)) {
    throw error
  }
  toolchainError = error
}
if (invocation) {
  console.log(
    `[parity] building + running orca-parity (${invocation.args.slice(0, 2).join(' ')}, offline via rust/vendor)`
  )
  rustStatus = run(invocation.command, invocation.args, {
    cwd: resolve(projectDir, 'rust'),
    env: invocation.env
  })
} else if (prebuilt) {
  console.warn(`[parity] ${toolchainError.message}`)
  console.warn(`[parity] no Rust toolchain; using prebuilt ${prebuilt} (may be stale)`)
  rustStatus = run(prebuilt, [vectorsDir, outputsFile])
} else {
  console.error(`[parity] ${toolchainError.message}`)
  console.error('[parity] and there is no prebuilt orca-parity to fall back to')
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
