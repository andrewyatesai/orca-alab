#!/usr/bin/env node

// Build the release orca-daemon binary (rust/target/release/orca-daemon, or
// orca-daemon.exe on Windows). The Rust daemon is THE terminal daemon on every
// platform with NO Node fallback, so its binary is a REQUIRED build artifact:
// electron-builder bundles it to Resources/orca-daemon(.exe) (rustDaemonResource /
// rustDaemonResourceWin) and fails the build if it is missing. This step produces
// it, guaranteeing "one correct solution that always works". On Windows the
// named-pipe transport (orca-winpipe) resolves fully offline via rust/vendor.
//
// Toolchain: the Trust toolchain, via `targo --unverified build` (owner directive
// 2026-09-28; no stock fallback). A triple Trust cannot build — the x64 slice of a
// universal mac build, linux/windows hosts — is a labelled STOCK EXCEPTION (see
// rust-toolchain-lane.mjs). Fully offline: the workspace resolves against rust/vendor.

import { chmodSync, copyFileSync, existsSync, mkdirSync } from 'node:fs'
import {
  clearInstalledAtermSourceCommit,
  readCleanAtermSourceCommit,
  writeInstalledAtermSourceCommit
} from './terminal-addon-source-stamp.mjs'
import { dirname, resolve } from 'node:path'
import {
  DARWIN_TRIPLES,
  lipoCreate,
  needsPerTargetMacBuild,
  resolveMacBuildArches
} from './mac-build-arches.mjs'
import { RustToolchainError, cargoInvocation } from './rust-toolchain-lane.mjs'
import { CargoCommandFailure, runStreamedCargoCommand } from './stream-cargo-command.mjs'

const projectDir = resolve(import.meta.dirname, '../..')
const manifest = resolve(projectDir, 'rust/Cargo.toml')
const rustWorkspaceDir = dirname(manifest)
// Cargo appends .exe on Windows; the packaged resource + resolver expect the same.
const binExt = process.platform === 'win32' ? '.exe' : ''
const binPath = resolve(projectDir, `rust/target/release/orca-daemon${binExt}`)
// Why: the daemon statically embeds its own aterm copy, so it needs the same source
// provenance the addon records — otherwise a stale daemon ships beside a fresh engine
// and no gate objects (see check-native-artifact-provenance.mjs).
const daemonStampPath = resolve(projectDir, 'rust/target/release/.orca-daemon-aterm-source.json')
const atermSource = resolve(projectDir, 'rust/aterm')

function stampDaemonProvenance() {
  const sourceCommit = readCleanAtermSourceCommit(atermSource)
  if (sourceCommit) {
    writeInstalledAtermSourceCommit(daemonStampPath, sourceCommit)
  } else {
    // A dirty or unreadable checkout has no exact provenance; an absent stamp is the
    // honest state and the checker reports it as such.
    clearInstalledAtermSourceCommit(daemonStampPath)
  }
}

// Returns the target dir the build wrote to (the stock-exception lane has its own).
async function runCargoBuild(triple) {
  // Cargo discovers `.cargo/config.toml` from the invocation directory, not from
  // `--manifest-path`. Run inside `rust/` so the checked-in offline vendor source
  // is actually used.
  const invocation = cargoInvocation({
    verb: 'build',
    args: ['--release', '-p', 'orca-daemon', '--manifest-path', manifest, '--offline'],
    triple,
    label: 'build-rust-daemon',
    // A cross-arch STOCK EXCEPTION slice builds beside Trust-built units on this
    // host, so it gets its own target dir; a host with no Trust toolchain keeps
    // the default one.
    stockTargetDir: triple ? resolve(rustWorkspaceDir, 'target/stock-exception') : null
  })
  await runStreamedCargoCommand({
    command: invocation.command,
    args: invocation.args,
    cwd: rustWorkspaceDir,
    env: invocation.env,
    label: 'build-rust-daemon'
  })
  return invocation.targetDir ?? resolve(rustWorkspaceDir, 'target')
}

async function main() {
  // Why: mac release/dual-arch builds must not package the host-arch binary
  // into the foreign-arch bundle (audit F2). Build per --target and lipo-merge
  // so the single static extraResources path covers every packaged arch. The
  // dev default stays a plain host-arch build (fast path, no extra targets).
  const macArches = process.platform === 'darwin' ? resolveMacBuildArches() : null
  if (macArches && needsPerTargetMacBuild(macArches)) {
    const perTargetBinPaths = []
    for (const arch of macArches) {
      const triple = DARWIN_TRIPLES[arch]
      console.log(`[build-rust-daemon] building release orca-daemon for ${triple} (offline)`)
      const targetDir = await runCargoBuild(triple)
      const targetBinPath = resolve(targetDir, `${triple}/release/orca-daemon`)
      if (!existsSync(targetBinPath)) {
        throw new CargoCommandFailure(`expected binary missing after build: ${targetBinPath}`)
      }
      perTargetBinPaths.push(targetBinPath)
    }
    // Why: per-target cargo builds emit under rust/target/<triple>/release, so
    // rust/target/release may not exist yet on a fresh clone.
    mkdirSync(dirname(binPath), { recursive: true })
    if (perTargetBinPaths.length === 1) {
      copyFileSync(perTargetBinPaths[0], binPath)
    } else {
      lipoCreate(perTargetBinPaths, binPath)
    }
    chmodSync(binPath, 0o755)
    stampDaemonProvenance()
    console.log(`[build-rust-daemon] built ${binPath} (${macArches.join(' + ')})`)
  } else {
    console.log(
      '[build-rust-daemon] building release orca-daemon (targo --unverified, offline via rust/vendor)'
    )
    await runCargoBuild(null)
    if (!existsSync(binPath)) {
      throw new CargoCommandFailure(`expected binary missing after build: ${binPath}`)
    }
    stampDaemonProvenance()
    console.log(`[build-rust-daemon] built ${binPath}`)
  }
}

try {
  await main()
} catch (error) {
  if (error instanceof RustToolchainError) {
    console.error(`[build-rust-daemon] ${error.message}`)
    process.exitCode = 1
  } else if (error instanceof CargoCommandFailure) {
    console.error(`[build-rust-daemon] ${error.message}`)
    process.exitCode = error.exitCode
  } else {
    throw error
  }
}
