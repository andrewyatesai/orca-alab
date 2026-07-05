import { describe, expect, it } from 'vitest'
import {
  DARWIN_TRIPLES,
  hostMacArch,
  needsPerTargetMacBuild,
  resolveMacBuildArches
} from './mac-build-arches.mjs'

describe('mac-build-arches', () => {
  it('defaults to host-arch-only (the dev fast path)', () => {
    expect(resolveMacBuildArches({}, 'arm64')).toEqual(['arm64'])
    expect(resolveMacBuildArches({}, 'x64')).toEqual(['x64'])
  })

  it('builds both arches on the mac release path', () => {
    expect(resolveMacBuildArches({ ORCA_MAC_RELEASE: '1' }, 'arm64')).toEqual(['x64', 'arm64'])
  })

  it('lets ORCA_MAC_BUILD_ARCHES override, trimming and deduping', () => {
    expect(resolveMacBuildArches({ ORCA_MAC_BUILD_ARCHES: 'x64, arm64' }, 'arm64')).toEqual([
      'x64',
      'arm64'
    ])
    expect(resolveMacBuildArches({ ORCA_MAC_BUILD_ARCHES: 'arm64,arm64' }, 'x64')).toEqual([
      'arm64'
    ])
  })

  it('rejects unknown arches instead of silently building the host arch', () => {
    expect(() => resolveMacBuildArches({ ORCA_MAC_BUILD_ARCHES: 'universal' }, 'arm64')).toThrow(
      /Unsupported mac build arch/
    )
  })

  // Why: any request a plain host `cargo build` cannot satisfy must switch to
  // per-target builds — that is the audit F2 fix.
  it('requires per-target builds for dual-arch or foreign-arch requests', () => {
    expect(needsPerTargetMacBuild(['x64', 'arm64'], 'arm64')).toBe(true)
    expect(needsPerTargetMacBuild(['x64'], 'arm64')).toBe(true)
    expect(needsPerTargetMacBuild(['arm64'], 'arm64')).toBe(false)
    expect(needsPerTargetMacBuild(['x64'], 'x64')).toBe(false)
  })

  it('maps node arch names to darwin triples', () => {
    expect(DARWIN_TRIPLES.x64).toBe('x86_64-apple-darwin')
    expect(DARWIN_TRIPLES.arm64).toBe('aarch64-apple-darwin')
    expect(hostMacArch('x64')).toBe('x64')
    expect(hostMacArch('arm64')).toBe('arm64')
  })
})
