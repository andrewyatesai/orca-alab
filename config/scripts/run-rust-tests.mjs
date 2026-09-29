#!/usr/bin/env node

// Test entrypoint for the rust/ workspace (`pnpm run test:rust`).
//
// Runs `targo --unverified test --workspace` — the Trust toolchain's fast lane,
// which asks whether the code passes its tests and makes no proof claim
// (`pnpm verify:rust` is the verification lane). No stock fallback: a missing
// targo fails naming targo (rust-toolchain-lane.mjs). Fully offline: the
// workspace resolves against rust/vendor.
//
// Extra CLI args pass straight through to `targo --unverified test`, so
// `pnpm run test:rust -- -p orca-core` narrows the run.

import { dirname, resolve } from 'node:path'
import { RustToolchainError, cargoInvocation } from './rust-toolchain-lane.mjs'
import { CargoCommandFailure, runStreamedCargoCommand } from './stream-cargo-command.mjs'

const projectDir = resolve(import.meta.dirname, '../..')
const manifest = resolve(projectDir, 'rust/Cargo.toml')
const rustWorkspaceDir = dirname(manifest)

async function main() {
  const invocation = cargoInvocation({
    verb: 'test',
    args: ['--workspace', '--manifest-path', manifest, '--offline', ...process.argv.slice(2)],
    label: 'test-rust'
  })
  console.log(`[test-rust] ${invocation.args.join(' ')} (offline via rust/vendor)`)
  // Run inside rust/ so cargo discovers rust/.cargo/config.toml and the
  // checked-in offline vendor source is actually used.
  await runStreamedCargoCommand({
    command: invocation.command,
    args: invocation.args,
    cwd: rustWorkspaceDir,
    env: invocation.env,
    label: 'test-rust'
  })
}

try {
  await main()
} catch (error) {
  if (error instanceof RustToolchainError) {
    console.error(`[test-rust] ${error.message}`)
    process.exitCode = 1
  } else if (error instanceof CargoCommandFailure) {
    console.error(`[test-rust] ${error.message}`)
    process.exitCode = error.exitCode
  } else {
    throw error
  }
}
