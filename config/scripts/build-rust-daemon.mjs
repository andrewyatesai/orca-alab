#!/usr/bin/env node

// Build the release orca-daemon binary (rust/target/release/orca-daemon, or
// orca-daemon.exe on Windows). The Rust daemon is THE terminal daemon on every
// platform with NO Node fallback, so its binary is a REQUIRED build artifact:
// electron-builder bundles it to Resources/orca-daemon(.exe) (rustDaemonResource /
// rustDaemonResourceWin) and fails the build if it is missing. This step produces
// it, guaranteeing "one correct solution that always works". On Windows the
// named-pipe transport (orca-winpipe) resolves fully offline via rust/vendor.
//
// Toolchain: the orca-crates workspace needs rustc 1.96, but the machine default
// cargo can be a Homebrew 1.95 shadow that also shadows its child rustc. Pin BOTH
// to the rustup `stable` toolchain (matching build-aterm-wasm.mjs / run-parity.mjs).
// Fully offline: the workspace resolves against rust/vendor.

import { spawnSync } from 'node:child_process'
import { chmodSync, copyFileSync, existsSync, mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import {
  DARWIN_TRIPLES,
  assertRustupDarwinTargetsInstalled,
  lipoCreate,
  needsPerTargetMacBuild,
  resolveMacBuildArches
} from './mac-build-arches.mjs'

const projectDir = resolve(import.meta.dirname, '../..')
const manifest = resolve(projectDir, 'rust/Cargo.toml')
// Cargo appends .exe on Windows; the packaged resource + resolver expect the same.
const binExt = process.platform === 'win32' ? '.exe' : ''
const binPath = resolve(projectDir, `rust/target/release/orca-daemon${binExt}`)

function rustupBin(tool) {
  const r = spawnSync('rustup', ['which', tool, '--toolchain', 'stable'], { encoding: 'utf8' })
  return r.status === 0 ? r.stdout.trim() : null
}

const cargoBin = rustupBin('cargo')
const rustcBin = rustupBin('rustc')
if (!cargoBin || !rustcBin) {
  console.error(
    '[build-rust-daemon] rustup `stable` toolchain unavailable (the workspace needs rustc 1.96). ' +
      'Install it with `rustup toolchain install stable`.'
  )
  process.exit(1)
}

function runCargoBuild(extraArgs) {
  const r = spawnSync(
    cargoBin,
    ['build', '--release', '-p', 'orca-daemon', '--manifest-path', manifest, '--offline', ...extraArgs],
    { stdio: 'inherit', cwd: projectDir, env: { ...process.env, RUSTC: rustcBin } }
  )
  if (r.status !== 0) {
    console.error(`[build-rust-daemon] cargo build failed (exit ${r.status})`)
    process.exit(r.status ?? 1)
  }
}

// Why: mac release/dual-arch builds must not package the host-arch binary
// into the foreign-arch bundle (audit F2). Build per --target and lipo-merge
// so the single static extraResources path covers every packaged arch. The
// dev default stays a plain host-arch build (fast path, no extra targets).
const macArches = process.platform === 'darwin' ? resolveMacBuildArches() : null
if (macArches && needsPerTargetMacBuild(macArches)) {
  assertRustupDarwinTargetsInstalled(macArches)
  const perTargetBinPaths = macArches.map((arch) => {
    const triple = DARWIN_TRIPLES[arch]
    console.log(`[build-rust-daemon] building release orca-daemon for ${triple} (offline)`)
    runCargoBuild(['--target', triple])
    const targetBinPath = resolve(projectDir, `rust/target/${triple}/release/orca-daemon`)
    if (!existsSync(targetBinPath)) {
      console.error(`[build-rust-daemon] expected binary missing after build: ${targetBinPath}`)
      process.exit(1)
    }
    return targetBinPath
  })
  // Why: per-target cargo builds emit under rust/target/<triple>/release, so
  // rust/target/release may not exist yet on a fresh clone.
  mkdirSync(dirname(binPath), { recursive: true })
  if (perTargetBinPaths.length === 1) {
    copyFileSync(perTargetBinPaths[0], binPath)
  } else {
    lipoCreate(perTargetBinPaths, binPath)
  }
  chmodSync(binPath, 0o755)
  console.log(`[build-rust-daemon] built ${binPath} (${macArches.join(' + ')})`)
} else {
  console.log(
    '[build-rust-daemon] building release orca-daemon (rustup stable, offline via rust/vendor)'
  )
  runCargoBuild([])
  if (!existsSync(binPath)) {
    console.error(`[build-rust-daemon] expected binary missing after build: ${binPath}`)
    process.exit(1)
  }
  console.log(`[build-rust-daemon] built ${binPath}`)
}
