// Maps Orca's macOS build-arch environment contract (ORCA_MAC_BUILD_ARCHES /
// ORCA_MAC_RELEASE) onto cargo target triples and lipo merging, so the cargo
// artifacts (orca-daemon, orca_node.node) always cover every arch
// electron-builder is about to package (staging-launch audit F2). The arch
// resolution here MUST mirror macTargetArches in config/electron-builder.config.cjs.

import { spawnSync } from 'node:child_process'

export const DARWIN_TRIPLES = {
  x64: 'x86_64-apple-darwin',
  arm64: 'aarch64-apple-darwin'
}

export function hostMacArch(processArch = process.arch) {
  return processArch === 'x64' ? 'x64' : 'arm64'
}

/**
 * Resolves the mac arches the current build must cover:
 * ORCA_MAC_BUILD_ARCHES (comma-separated) wins, ORCA_MAC_RELEASE=1 means both,
 * otherwise host-arch-only (the dev default).
 */
export function resolveMacBuildArches(env = process.env, processArch = process.arch) {
  const requested = env.ORCA_MAC_BUILD_ARCHES
    ? env.ORCA_MAC_BUILD_ARCHES.split(',')
        .map((arch) => arch.trim())
        .filter(Boolean)
    : env.ORCA_MAC_RELEASE === '1'
      ? ['x64', 'arm64']
      : [hostMacArch(processArch)]
  const unknown = requested.filter((arch) => !DARWIN_TRIPLES[arch])
  if (unknown.length > 0) {
    throw new Error(
      `Unsupported mac build arch(es): ${unknown.join(', ')} (supported: x64, arm64)`
    )
  }
  return [...new Set(requested)]
}

/** True when a plain host-arch `cargo build` cannot satisfy the request. */
export function needsPerTargetMacBuild(arches, processArch = process.arch) {
  return arches.length > 1 || arches[0] !== hostMacArch(processArch)
}

/** Fails fast with an actionable message when rustup lacks a needed std. */
export function assertRustupDarwinTargetsInstalled(arches) {
  const result = spawnSync('rustup', ['target', 'list', '--installed', '--toolchain', 'stable'], {
    encoding: 'utf8'
  })
  if (result.status !== 0) {
    throw new Error(
      'rustup is required for cross-arch mac builds (`rustup target list` failed). ' +
        'Install rustup with a stable toolchain first.'
    )
  }
  const installed = new Set(result.stdout.split('\n').map((line) => line.trim()))
  const missing = arches.map((arch) => DARWIN_TRIPLES[arch]).filter((t) => !installed.has(t))
  if (missing.length > 0) {
    throw new Error(
      `Missing rust std for target(s): ${missing.join(', ')}. ` +
        `Run: rustup target add ${missing.join(' ')} --toolchain stable`
    )
  }
}

/** Reads the arches a Mach-O (thin or fat) file covers, in Node arch names. */
export function machOFileArches(filePath) {
  const result = spawnSync('lipo', ['-archs', filePath], { encoding: 'utf8' })
  if (result.status !== 0) {
    return []
  }
  return result.stdout
    .trim()
    .split(/\s+/)
    .filter(Boolean)
    .map((token) => (token === 'x86_64' || token === 'x86_64h' ? 'x64' : token))
}

/** lipo-merges per-target outputs into one universal file (or copies a single one). */
export function lipoCreate(inputs, output) {
  const result = spawnSync('lipo', ['-create', ...inputs, '-output', output], {
    stdio: 'inherit'
  })
  if (result.status !== 0) {
    throw new Error(`lipo -create failed (exit ${result.status}) for ${output}`)
  }
}
