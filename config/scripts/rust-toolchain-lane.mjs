// The one place Orca's scripts choose a Rust build tool.
//
// ALab systems build with the Trust toolchain (owner directive 2026-09-28):
// `targo` drives the build and `trustc` compiles. Every cargo verb a script
// runs goes through `targo --unverified <verb>` (the fast lane, no proof claim)
// unless the script's purpose is verification, which uses `targo trust`.
// There is NO stock fallback for a target Trust serves: a missing targo is an
// error that names targo, never a quiet switch to rustup's cargo.
//
// STOCK EXCEPTION (Trust lacks a std for every triple but aarch64-apple-darwin;
// measured 2026-09-28 against targo 1.99.0-dev 321aaeda7: wasm32-unknown-unknown
// fails E0463 "can't find crate for `core`", x86_64-apple-darwin and the
// linux/windows triples fail E0463 "can't find crate for `std`", and
// -Zbuild-std fails because the seal ships no rust-src). Builds for those
// triples keep using an INSTALLED stock rustup toolchain, selected by
// ORCA_STOCK_RUST_TOOLCHAIN (default `stable`). This module never lets rustup
// install anything: `rustup which` auto-installs a missing channel from the
// network, so the toolchain and its targets are checked with read-only
// `rustup toolchain list` / `rustup target list --installed` first, and every
// rustup call runs with RUSTUP_AUTO_INSTALL=0.

import { spawnSync } from 'node:child_process'

/** Triples the installed Trust seal ships a std for (lib/rustlib/<triple>). */
export const TRUST_TRIPLES = new Set(['aarch64-apple-darwin'])

const HOST_TRIPLES = {
  'darwin-arm64': 'aarch64-apple-darwin',
  'darwin-x64': 'x86_64-apple-darwin',
  'linux-arm64': 'aarch64-unknown-linux-gnu',
  'linux-x64': 'x86_64-unknown-linux-gnu',
  'win32-arm64': 'aarch64-pc-windows-msvc',
  'win32-x64': 'x86_64-pc-windows-msvc'
}

export function hostTriple(platform = process.platform, arch = process.arch) {
  return HOST_TRIPLES[`${platform}-${arch}`] ?? `${arch}-${platform}`
}

export function trustServes(triple) {
  return TRUST_TRIPLES.has(triple)
}

export class RustToolchainError extends Error {}

const TARGO_HINT =
  'Install the Trust toolchain (atpkg ships targo/trustc) and put `targo` on PATH. ' +
  'There is no stock fallback for this target.'

/**
 * targo, proven to be targo. `$CARGO` wins when a parent targo exported it.
 * `targo --unverified --version` is the discriminator: upstream cargo —
 * including rustup's `trust` cargo — rejects `--unverified`.
 */
export function resolveTargo(env = process.env) {
  const command = env.CARGO || 'targo'
  const probe = spawnSync(command, ['--unverified', '--version'], { encoding: 'utf8', env })
  if (probe.error) {
    throw new RustToolchainError(
      `could not run \`${command}\` (${probe.error.message}). ${TARGO_HINT}`
    )
  }
  if (probe.status !== 0) {
    const why = (probe.stderr || probe.stdout || '').trim().split('\n')[0]
    throw new RustToolchainError(
      `\`${command}\` is not targo (\`${command} --unverified --version\` exited ${probe.status}: ${why}). ${TARGO_HINT}`
    )
  }
  return { command, version: probe.stdout.trim() }
}

/** targo argv for `verb` in the unverified lane. */
export function targoUnverifiedArgs(verb, args = []) {
  return ['--unverified', verb, ...args]
}

function rustup(args, env) {
  return spawnSync('rustup', args, {
    encoding: 'utf8',
    env: { ...env, RUSTUP_AUTO_INSTALL: '0' }
  })
}

/**
 * STOCK EXCEPTION toolchain for a triple Trust cannot build. Returns
 * { toolchain, cargo, env } with RUSTC and RUSTUP_TOOLCHAIN pinned, or throws
 * naming exactly what to install. Never installs anything itself.
 */
export function stockExceptionToolchain({ targets = [], env = process.env } = {}) {
  const toolchain = env.ORCA_STOCK_RUST_TOOLCHAIN || 'stable'
  const listed = rustup(['toolchain', 'list'], env)
  if (listed.error || listed.status !== 0) {
    throw new RustToolchainError(
      `STOCK EXCEPTION lane needs rustup (for ${targets.join(', ') || 'this target'}, which Trust cannot build), but \`rustup toolchain list\` failed.`
    )
  }
  const installed = listed.stdout
    .split('\n')
    .map((line) => line.trim().split(/\s+/)[0])
    .filter(Boolean)
  if (!installed.some((name) => name === toolchain || name.startsWith(`${toolchain}-`))) {
    throw new RustToolchainError(
      `STOCK EXCEPTION lane needs the rustup toolchain \`${toolchain}\`, which is not installed ` +
        `(installed: ${installed.join(', ') || 'none'}). This script never installs toolchains: ` +
        `run \`rustup toolchain install ${toolchain}${targets.length ? ` --target ${targets.join(',')}` : ''}\` ` +
        'yourself, or set ORCA_STOCK_RUST_TOOLCHAIN to an installed stock toolchain.'
    )
  }
  if (targets.length > 0) {
    const listedTargets = rustup(['target', 'list', '--installed', '--toolchain', toolchain], env)
    const have = new Set((listedTargets.stdout ?? '').split('\n').map((line) => line.trim()))
    const missing = targets.filter((target) => !have.has(target))
    if (listedTargets.status !== 0 || missing.length > 0) {
      throw new RustToolchainError(
        `STOCK EXCEPTION toolchain \`${toolchain}\` lacks the std for ${missing.join(', ') || targets.join(', ')}. ` +
          `Run: rustup target add ${(missing.length ? missing : targets).join(' ')} --toolchain ${toolchain}`
      )
    }
  }
  const which = (tool) => {
    const r = rustup(['which', tool, '--toolchain', toolchain], env)
    if (r.status !== 0) {
      throw new RustToolchainError(
        `\`rustup which ${tool} --toolchain ${toolchain}\` failed: ${(r.stderr || '').trim().split('\n')[0]}`
      )
    }
    return r.stdout.trim()
  }
  return {
    toolchain,
    cargo: which('cargo'),
    env: {
      ...env,
      RUSTC: which('rustc'),
      RUSTUP_TOOLCHAIN: toolchain,
      RUSTUP_AUTO_INSTALL: '0'
    }
  }
}

/**
 * The cargo invocation for `verb args` compiling for `triple` (null = host).
 * Trust-served triples get targo's unverified lane; any other triple is a
 * labelled STOCK EXCEPTION. `--target` is added only when `triple` is given.
 * `stockTargetDir`, when given, becomes CARGO_TARGET_DIR for the stock lane
 * only, so stock- and Trust-compiled host units never share a deps/ dir.
 * Returns { lane, command, args, env, targetDir } (targetDir null = default).
 */
export function cargoInvocation({
  verb,
  args = [],
  triple = null,
  env = process.env,
  label,
  stockTargetDir = null
}) {
  const compileTriple = triple ?? hostTriple()
  const targetArgs = triple ? ['--target', triple] : []
  if (trustServes(compileTriple)) {
    const { command } = resolveTargo(env)
    return {
      lane: 'trust',
      command,
      args: targoUnverifiedArgs(verb, [...args, ...targetArgs]),
      env,
      targetDir: null
    }
  }
  const stock = stockExceptionToolchain({ targets: [compileTriple], env })
  console.log(
    `[${label}] STOCK EXCEPTION: Trust ships no std for ${compileTriple}; ` +
      `building with the installed rustup toolchain \`${stock.toolchain}\`.`
  )
  return {
    lane: 'stock-exception',
    command: stock.cargo,
    args: [verb, ...args, ...targetArgs],
    env: stockTargetDir ? { ...stock.env, CARGO_TARGET_DIR: stockTargetDir } : stock.env,
    targetDir: stockTargetDir
  }
}
