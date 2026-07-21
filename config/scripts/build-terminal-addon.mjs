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
import { CargoCommandFailure, runStreamedCargoCommand } from './stream-cargo-command.mjs'
import {
  clearInstalledAtermSourceCommit,
  installedAtermSourceIsCurrent,
  readCleanAtermSourceCommit,
  writeInstalledAtermSourceCommit
} from './terminal-addon-source-stamp.mjs'

const projectDir = resolve(import.meta.dirname, '../..')
const addonDir = resolve(projectDir, 'native/orca-node')
const atermSource = resolve(projectDir, 'rust/aterm')
const ADDON_NAME = 'orca_node.node'
const ATERM_SOURCE_STAMP = resolve(addonDir, 'target/.orca-installed-aterm-source.json')
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
  const r = spawnSync('rustup', ['which', tool, '--toolchain', RUST_TOOLCHAIN], {
    encoding: 'utf8'
  })
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
    resolve(projectDir, 'rust/crates/orca-terminal/Cargo.toml')
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
  if (
    existsSync(dest) &&
    statSync(dest).mtimeMs >= newestMtime() &&
    destCoversRequestedArches() &&
    installedAtermSourceIsCurrent(atermSource, ATERM_SOURCE_STAMP)
  ) {
    console.log('[terminal-addon] addon up to date; skipping rebuild.')
    process.exit(0)
  }
}

ensureAtermSubmodule()
const buildAtermSourceCommit = readCleanAtermSourceCommit(atermSource)
// Clear before Cargo starts: if a rebuild fails, a prior stamp must not make the
// surviving old addon look current on the next invocation.
clearInstalledAtermSourceCommit(ATERM_SOURCE_STAMP)

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

async function runCargoBuild(targetTriple) {
  const args = ['build', '--release', ...(targetTriple ? ['--target', targetTriple] : [])]
  try {
    await runStreamedCargoCommand({
      command: stableCargo ?? 'cargo',
      args,
      cwd: addonDir,
      env,
      label: 'terminal-addon',
      // shell only for bare-name PATH lookup; an absolute cargo path may contain spaces.
      shell: process.platform === 'win32' && !stableCargo
    })
  } catch (error) {
    if (error instanceof CargoCommandFailure && error.reason === 'spawn') {
      console.error(
        '[terminal-addon] Install rustup with a stable toolchain >=1.96 (https://rustup.rs), then re-run.'
      )
    }
    throw error
  }
  const built = resolve(
    addonDir,
    targetTriple ? `target/${targetTriple}/release` : 'target/release',
    cdylibName()
  )
  if (!existsSync(built)) {
    throw new CargoCommandFailure(`expected cargo artifact missing: ${built}`)
  }
  return built
}

async function main() {
  if (perTargetMacBuild) {
    assertRustupDarwinTargetsInstalled(macArches)
    const perTargetArtifacts = []
    for (const arch of macArches) {
      const triple = DARWIN_TRIPLES[arch]
      console.log(`[terminal-addon] building aterm napi addon for ${triple}…`)
      perTargetArtifacts.push(await runCargoBuild(triple))
    }
    if (perTargetArtifacts.length === 1) {
      copyFileSync(perTargetArtifacts[0], dest)
    } else {
      lipoCreate(perTargetArtifacts, dest)
    }
    console.log(`[terminal-addon] installed ${dest} (${macArches.join(' + ')})`)
  } else {
    console.log('[terminal-addon] building aterm napi addon (cargo build --release)…')
    const built = await runCargoBuild(null)
    copyFileSync(built, dest)
    console.log(`[terminal-addon] installed ${dest}`)
  }

  const installedAtermSourceCommit = readCleanAtermSourceCommit(atermSource)
  if (buildAtermSourceCommit && installedAtermSourceCommit === buildAtermSourceCommit) {
    writeInstalledAtermSourceCommit(ATERM_SOURCE_STAMP, buildAtermSourceCommit)
    console.log(`[terminal-addon] stamped aterm source ${buildAtermSourceCommit.slice(0, 12)}`)
  } else {
    // Dirty source and a checkout that changes while Cargo runs are both valid
    // for local experimentation, but neither has a trustworthy commit identity.
    console.log(
      '[terminal-addon] aterm source is dirty, changed, or unidentified; addon remains uncacheable.'
    )
  }
}

try {
  await main()
} catch (error) {
  if (!(error instanceof CargoCommandFailure)) {
    throw error
  }
  console.error(`[terminal-addon] ${error.message}`)
  process.exitCode = error.exitCode
}
