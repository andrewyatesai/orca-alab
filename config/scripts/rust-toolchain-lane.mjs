// The one place Orca's scripts choose a Rust build tool.
//
// ALab systems build with the Trust toolchain (owner directive 2026-09-28):
// `targo` drives the build and `trustc` compiles. Every cargo verb a script
// runs goes through `targo --unverified <verb>` (the fast lane, no proof claim)
// unless the script's purpose is verification, which uses `targo trust`.
//
// The lane is decided PER RUN from the toolchain actually installed, never from
// a hard-coded triple list: a triple builds with targo whenever targo resolves
// AND the trustc it drives ships that triple's std (`libstd-*.rlib` under
// `trustc --print target-libdir --target <triple>`; targo honours `$RUSTC`, so
// the probe does too). On a TRUST-REQUIRED triple (the host the Trust seal is
// measured to serve) a failed probe is an error that names targo — there is NO
// stock fallback there.
//
// STOCK EXCEPTION: any other triple whose probe fails keeps using an INSTALLED
// stock rustup toolchain (ORCA_STOCK_RUST_TOOLCHAIN, default `stable`), and the
// log line carries the probe result. Measured 2026-09-28 against targo
// 1.99.0-dev 321aaeda7: the atpkg seal ships lib/rustlib/aarch64-apple-darwin
// only, so wasm32-unknown-unknown fails E0463 "can't find crate for `core`",
// x86_64-apple-darwin and the linux/windows triples fail E0463 "can't find crate
// for `std`", and -Zbuild-std fails because the seal ships no rust-src. The
// exception retires by itself once the installed Trust toolchain ships the std.
// This module never lets rustup install anything: `rustup which` auto-installs a
// missing channel from the network, so the toolchain and its targets are checked
// with read-only `rustup toolchain list` / `rustup target list --installed`
// first, and every rustup call runs with RUSTUP_AUTO_INSTALL=0.

import { spawnSync } from 'node:child_process'
import { readdirSync } from 'node:fs'

/**
 * Triples on which a missing or broken Trust toolchain is an ERROR, never a
 * stock fallback: the host the installed Trust seal is measured to serve.
 */
export const TRUST_REQUIRED_TRIPLES = new Set(['aarch64-apple-darwin'])

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

export class RustToolchainError extends Error {}

const TARGO_HINT =
  'Install the Trust toolchain (atpkg ships targo/trustc) and put `targo` on PATH. ' +
  'There is no stock fallback for this target.'

const firstLine = (text) => (text || '').trim().split('\n')[0]

// targo, proven to be targo, or the reason it is not. `$CARGO` wins when a
// parent targo exported it. `targo --unverified --version` is the discriminator:
// upstream cargo — including rustup's `trust` cargo — rejects `--unverified`.
function probeTargo(env) {
  const command = env.CARGO || 'targo'
  const probe = spawnSync(command, ['--unverified', '--version'], { encoding: 'utf8', env })
  if (probe.error) {
    return { command: null, why: `could not run \`${command}\` (${probe.error.message})` }
  }
  if (probe.status !== 0) {
    const why = firstLine(probe.stderr || probe.stdout)
    return {
      command: null,
      why: `\`${command}\` is not targo (\`${command} --unverified --version\` exited ${probe.status}: ${why})`
    }
  }
  return { command, version: probe.stdout.trim(), why: null }
}

/** targo, proven to be targo; throws RustToolchainError naming targo otherwise. */
export function resolveTargo(env = process.env) {
  const targo = probeTargo(env)
  if (!targo.command) {
    throw new RustToolchainError(`${targo.why}. ${TARGO_HINT}`)
  }
  return { command: targo.command, version: targo.version }
}

/**
 * Whether the INSTALLED Trust toolchain can build `triple`. Returns
 * { served, targo, detail }; `detail` names what was measured.
 */
export function probeTrustTriple(triple, env = process.env) {
  const targo = probeTargo(env)
  if (!targo.command) {
    return { served: false, targo: null, detail: targo.why }
  }
  const trustc = env.RUSTC || 'trustc'
  const libdir = spawnSync(trustc, ['--print', 'target-libdir', '--target', triple], {
    encoding: 'utf8',
    env
  })
  if (libdir.error || libdir.status !== 0) {
    const why = libdir.error ? libdir.error.message : firstLine(libdir.stderr)
    return {
      served: false,
      targo: targo.command,
      detail: `\`${trustc} --print target-libdir --target ${triple}\` failed: ${why}`
    }
  }
  const dir = libdir.stdout.trim()
  let entries = []
  try {
    entries = readdirSync(dir)
  } catch {
    entries = []
  }
  if (!entries.some((name) => /^libstd-.*\.rlib$/.test(name))) {
    return {
      served: false,
      targo: targo.command,
      detail: `the installed Trust toolchain ships no std for ${triple} (no libstd-*.rlib in ${dir})`
    }
  }
  return { served: true, targo: targo.command, detail: `Trust std for ${triple} in ${dir}` }
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
  const forTargets = targets.join(', ') || 'this target'
  const listed = rustup(['toolchain', 'list'], env)
  if (listed.error || listed.status !== 0) {
    throw new RustToolchainError(
      `STOCK EXCEPTION lane needs rustup (for ${forTargets}, which Trust cannot build), ` +
        'but `rustup toolchain list` failed.'
    )
  }
  const installed = listed.stdout
    .split('\n')
    .map((line) => line.trim().split(/\s+/)[0])
    .filter(Boolean)
  if (!installed.some((name) => name === toolchain || name.startsWith(`${toolchain}-`))) {
    const withTargets = targets.length ? ` --target ${targets.join(',')}` : ''
    throw new RustToolchainError(
      `STOCK EXCEPTION lane needs the rustup toolchain \`${toolchain}\`, which is not installed ` +
        `(installed: ${installed.join(', ') || 'none'}). This script never installs toolchains: ` +
        `run \`rustup toolchain install ${toolchain}${withTargets}\` ` +
        'yourself, or set ORCA_STOCK_RUST_TOOLCHAIN to an installed stock toolchain.'
    )
  }
  if (targets.length > 0) {
    const listedTargets = rustup(['target', 'list', '--installed', '--toolchain', toolchain], env)
    const have = new Set((listedTargets.stdout ?? '').split('\n').map((line) => line.trim()))
    const missing = targets.filter((target) => !have.has(target))
    if (listedTargets.status !== 0 || missing.length > 0) {
      const add = missing.length ? missing : targets
      throw new RustToolchainError(
        `STOCK EXCEPTION toolchain \`${toolchain}\` lacks the std for ${add.join(', ')}. ` +
          `Run: rustup target add ${add.join(' ')} --toolchain ${toolchain}`
      )
    }
  }
  const which = (tool) => {
    const r = rustup(['which', tool, '--toolchain', toolchain], env)
    if (r.status !== 0) {
      throw new RustToolchainError(
        `\`rustup which ${tool} --toolchain ${toolchain}\` failed: ${firstLine(r.stderr)}`
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
 * The build tool for compiling `triple`: targo's unverified lane when the
 * installed Trust toolchain ships that triple's std, else (never on a
 * TRUST-REQUIRED triple) the labelled STOCK EXCEPTION. Returns
 * { lane, command, laneArgs, env, detail }; `laneArgs` goes before the verb.
 */
export function toolchainForTriple({ triple, env = process.env, label, announce = true }) {
  const probe = probeTrustTriple(triple, env)
  if (probe.served) {
    return {
      lane: 'trust',
      command: probe.targo,
      laneArgs: ['--unverified'],
      env,
      detail: probe.detail
    }
  }
  if (TRUST_REQUIRED_TRIPLES.has(triple)) {
    throw new RustToolchainError(`${probe.detail}. ${TARGO_HINT}`)
  }
  const stock = stockExceptionToolchain({ targets: [triple], env })
  if (announce) {
    console.log(
      `[${label}] STOCK EXCEPTION for ${triple}: ${probe.detail}; ` +
        `building with the installed rustup toolchain \`${stock.toolchain}\`.`
    )
  }
  return {
    lane: 'stock-exception',
    command: stock.cargo,
    laneArgs: [],
    env: stock.env,
    detail: probe.detail
  }
}

// `--target` belongs to cargo, so it goes before a `--` that hands the rest of
// argv to the program being run.
function withTargetArgs(args, targetArgs) {
  const separator = args.indexOf('--')
  return separator === -1
    ? [...args, ...targetArgs]
    : [...args.slice(0, separator), ...targetArgs, ...args.slice(separator)]
}

/**
 * The cargo invocation for `verb args` compiling for `triple` (null = host).
 * `--target` is added only when `triple` is given. `stockTargetDir`, when
 * given, becomes CARGO_TARGET_DIR for the stock lane only, so stock- and
 * Trust-compiled host units never share a deps/ dir.
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
  const tool = toolchainForTriple({ triple: triple ?? hostTriple(), env, label })
  const verbArgs = [verb, ...withTargetArgs(args, triple ? ['--target', triple] : [])]
  if (tool.lane === 'trust') {
    return {
      lane: tool.lane,
      command: tool.command,
      args: [...tool.laneArgs, ...verbArgs],
      env: tool.env,
      targetDir: null
    }
  }
  return {
    lane: tool.lane,
    command: tool.command,
    args: verbArgs,
    env: stockTargetDir ? { ...tool.env, CARGO_TARGET_DIR: stockTargetDir } : tool.env,
    targetDir: stockTargetDir
  }
}
