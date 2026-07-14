#!/usr/bin/env node

// Builds the aterm-backed terminal engine as a Node-API addon (native/orca-node)
// and installs it as `orca_node.node`, the name the daemon's loader requires.
// aterm is the sole headless terminal engine, so this addon is mandatory: the
// daemon throws without it. Run before `test`, `build`, and `build:release`.

import { spawnSync } from 'node:child_process'
import { existsSync, copyFileSync, statSync, readdirSync } from 'node:fs'
import { resolve, join } from 'node:path'
import { homedir } from 'node:os'
import {
  DARWIN_TRIPLES,
  assertRustupDarwinTargetsInstalled,
  lipoCreate,
  machOFileArches,
  needsPerTargetMacBuild,
  resolveMacBuildArches
} from './mac-build-arches.mjs'

const projectDir = resolve(import.meta.dirname, '../..')
const addonDir = resolve(projectDir, 'native/orca-node')
const ADDON_NAME = 'orca_node.node'
// Skip the cargo fingerprint pass when the installed addon is already newer than
// every input the build would touch; `--force` always rebuilds.
const ifMissing = process.argv.includes('--if-missing')
const force = process.argv.includes('--force')
// Why: mac release/dual-arch packaging must not ship the host-arch addon in
// the foreign-arch bundle (audit F2); per-target cargo builds + lipo keep the
// single static extraResources path valid for every packaged arch.
const macArches = process.platform === 'darwin' ? resolveMacBuildArches() : null
const perTargetMacBuild = macArches !== null && needsPerTargetMacBuild(macArches)

function cdylibName() {
  // cargo's `cdylib` output name differs per platform; the loader requires the
  // single fixed name, so we copy/rename after the build.
  if (process.platform === 'darwin') {
    return 'liborca_node.dylib'
  }
  if (process.platform === 'win32') {
    return 'orca_node.dll'
  }
  return 'liborca_node.so'
}

function cargoEnv() {
  // Prefer the rustup toolchain (~/.cargo/bin) over an older system cargo so the
  // build gets a rustc new enough for aterm's edition-2024 crates.
  const cargoBin = join(homedir(), '.cargo', 'bin')
  const sep = process.platform === 'win32' ? ';' : ':'
  const path = existsSync(cargoBin)
    ? `${cargoBin}${sep}${process.env.PATH ?? ''}`
    : process.env.PATH
  return { ...process.env, PATH: path }
}

// Defaults to STABLE (the proven addon-build toolchain); ORCA_RUST_TOOLCHAIN=trust
// rebuilds the napi addon with the Trust-verified compiler.
const RUST_TOOLCHAIN = process.env.ORCA_RUST_TOOLCHAIN || 'stable'

function rustupStableBin(tool) {
  const r = spawnSync('rustup', ['which', tool, '--toolchain', RUST_TOOLCHAIN], { encoding: 'utf8' })
  return r.status === 0 ? r.stdout.trim() : null
}

function ensureAtermSubmodule() {
  // A default `git clone` leaves the rust/aterm submodule empty, and cargo's
  // path-dep error for that is cryptic — init it here so fresh clones just work.
  if (existsSync(resolve(projectDir, 'rust/aterm/Cargo.toml'))) {
    return
  }
  console.log(
    '[terminal-addon] rust/aterm submodule is empty — running `git submodule update --init rust/aterm`…'
  )
  const init = spawnSync('git', ['submodule', 'update', '--init', 'rust/aterm'], {
    cwd: projectDir,
    stdio: 'inherit'
  })
  if (
    init.error ||
    init.status !== 0 ||
    !existsSync(resolve(projectDir, 'rust/aterm/Cargo.toml'))
  ) {
    console.error(
      '[terminal-addon] failed to initialize the rust/aterm submodule (git and network access are required).'
    )
    console.error(
      '[terminal-addon] Run `git submodule update --init rust/aterm` manually, then re-run this build.'
    )
    process.exit(1)
  }
}

function newestMtime() {
  // Cheap freshness probe: the newest mtime among the addon + adapter sources.
  const roots = [
    resolve(addonDir, 'src'),
    resolve(addonDir, 'Cargo.toml'),
    resolve(projectDir, 'rust/crates/orca-terminal/src'),
    resolve(projectDir, 'rust/crates/orca-terminal/Cargo.toml'),
    // aterm is a pinned git submodule; its Cargo.lock changes on an engine bump
    // and `git checkout` refreshes the file's mtime — a cheap proxy for the whole
    // engine tree so a fresh pin triggers a rebuild without walking 900+ files.
    // (The bump script also rebuilds with --force, so this is just a fast path.)
    resolve(projectDir, 'rust/aterm/Cargo.lock')
  ]
  let newest = 0
  const walk = (p) => {
    if (!existsSync(p)) {
      return
    }
    const st = statSync(p)
    if (st.isDirectory()) {
      for (const e of readdirSync(p)) {
        walk(join(p, e))
      }
    } else {
      newest = Math.max(newest, st.mtimeMs)
    }
  }
  for (const r of roots) {
    walk(r)
  }
  return newest
}

const dest = resolve(addonDir, ADDON_NAME)

function destCoversRequestedArches() {
  // Why: an addon left over from a host-arch dev build looks "fresh" by mtime
  // but is unusable for a dual-arch package; require the requested slices too.
  if (macArches === null) {
    return true
  }
  const covered = machOFileArches(dest)
  return macArches.every((arch) => covered.includes(arch))
}

if (!force) {
  if (ifMissing && existsSync(dest) && destCoversRequestedArches()) {
    console.log('[terminal-addon] addon present; skipping (--if-missing).')
    process.exit(0)
  }
  if (existsSync(dest) && statSync(dest).mtimeMs >= newestMtime() && destCoversRequestedArches()) {
    console.log('[terminal-addon] addon up to date; skipping rebuild.')
    process.exit(0)
  }
}

ensureAtermSubmodule()

// Pin BOTH cargo and rustc to rustup's stable toolchain (matches run-parity.mjs):
// a Homebrew cargo on PATH ignores rust-toolchain.toml, and even rustup's cargo
// spawns a bare `rustc` from PATH unless RUSTC is pinned. Falls back to plain
// `cargo` (with ~/.cargo/bin prepended) when rustup is absent.
const stableCargo = rustupStableBin('cargo')
const stableRustc = rustupStableBin('rustc')
const env = cargoEnv()
if (stableRustc) {
  env.RUSTC = stableRustc
}

function runCargoBuild(targetTriple) {
  const args = ['build', '--release', ...(targetTriple ? ['--target', targetTriple] : [])]
  const build = spawnSync(stableCargo ?? 'cargo', args, {
    cwd: addonDir,
    env,
    stdio: 'inherit',
    // shell only for bare-name PATH lookup; an absolute cargo path may contain spaces.
    shell: process.platform === 'win32' && !stableCargo
  })
  if (build.error) {
    console.error(`[terminal-addon] cargo failed to start: ${build.error.message}`)
    console.error(
      '[terminal-addon] Install rustup with a stable toolchain >=1.96 (https://rustup.rs), then re-run.'
    )
    process.exit(1)
  }
  if (build.status !== 0) {
    process.exit(build.status ?? 1)
  }
  const built = resolve(
    addonDir,
    targetTriple ? `target/${targetTriple}/release` : 'target/release',
    cdylibName()
  )
  if (!existsSync(built)) {
    console.error(`[terminal-addon] expected cargo artifact missing: ${built}`)
    process.exit(1)
  }
  return built
}

if (perTargetMacBuild) {
  assertRustupDarwinTargetsInstalled(macArches)
  const perTargetArtifacts = macArches.map((arch) => {
    const triple = DARWIN_TRIPLES[arch]
    console.log(`[terminal-addon] building aterm napi addon for ${triple}…`)
    return runCargoBuild(triple)
  })
  if (perTargetArtifacts.length === 1) {
    copyFileSync(perTargetArtifacts[0], dest)
  } else {
    lipoCreate(perTargetArtifacts, dest)
  }
  console.log(`[terminal-addon] installed ${dest} (${macArches.join(' + ')})`)
} else {
  console.log('[terminal-addon] building aterm napi addon (cargo build --release)…')
  const built = runCargoBuild(null)
  copyFileSync(built, dest)
  console.log(`[terminal-addon] installed ${dest}`)
}
