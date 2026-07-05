import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

const require = createRequire(import.meta.url)
const {
  assertBundledBinaryArchitectures,
  readElfArchName,
  readMachOArchNames,
  readPeArchName
} = require('./assert-bundled-binary-arch.cjs')

function elfHeader(machine) {
  const header = Buffer.alloc(64)
  header.writeUInt32BE(0x7f454c46, 0)
  header[4] = 2 // ELFCLASS64
  header[5] = 1 // little-endian
  header.writeUInt16LE(machine, 18)
  return header
}

function peHeader(machine) {
  const buffer = Buffer.alloc(0x100)
  buffer.writeUInt16LE(0x5a4d, 0) // MZ
  buffer.writeUInt32LE(0x80, 0x3c) // e_lfanew
  buffer.writeUInt32LE(0x00004550, 0x80) // PE\0\0
  buffer.writeUInt16LE(machine, 0x84)
  return buffer
}

async function withTempDir(run) {
  const dir = await mkdtemp(join(tmpdir(), 'orca-bundle-arch-'))
  try {
    return await run(dir)
  } finally {
    await rm(dir, { recursive: true, force: true })
  }
}

describe('assert-bundled-binary-arch', () => {
  it('reads ELF machine names', async () => {
    await withTempDir(async (dir) => {
      const x64Path = join(dir, 'x64.bin')
      const arm64Path = join(dir, 'arm64.bin')
      await writeFile(x64Path, elfHeader(0x3e))
      await writeFile(arm64Path, elfHeader(0xb7))
      expect(readElfArchName(x64Path)).toBe('x64')
      expect(readElfArchName(arm64Path)).toBe('arm64')
    })
  })

  it('reads PE machine names', async () => {
    await withTempDir(async (dir) => {
      const x64Path = join(dir, 'x64.node')
      const arm64Path = join(dir, 'arm64.node')
      await writeFile(x64Path, peHeader(0x8664))
      await writeFile(arm64Path, peHeader(0xaa64))
      expect(readPeArchName(x64Path)).toBe('x64')
      expect(readPeArchName(arm64Path)).toBe('arm64')
    })
  })

  it.runIf(process.platform === 'darwin')('reads Mach-O arches via lipo', () => {
    const hostArch = process.arch === 'x64' ? 'x64' : 'arm64'
    expect(readMachOArchNames(process.execPath)).toContain(hostArch)
  })

  it('passes when the bundled linux binaries match the bundle arch', async () => {
    await withTempDir(async (dir) => {
      await writeFile(join(dir, 'orca-daemon'), elfHeader(0x3e))
      await writeFile(join(dir, 'orca_node.node'), elfHeader(0x3e))
      expect(() =>
        assertBundledBinaryArchitectures({
          resourcesDir: dir,
          electronPlatformName: 'linux',
          arch: 'x64'
        })
      ).not.toThrow()
    })
  })

  // Why: the exact audit F2 failure — a host-arch cargo binary copied into a
  // foreign-arch bundle must fail the build instead of shipping broken.
  it('fails when a bundled binary is built for a different arch', async () => {
    await withTempDir(async (dir) => {
      await writeFile(join(dir, 'orca-daemon'), elfHeader(0xb7)) // arm64
      await writeFile(join(dir, 'orca_node.node'), elfHeader(0xb7))
      expect(() =>
        assertBundledBinaryArchitectures({
          resourcesDir: dir,
          electronPlatformName: 'linux',
          arch: 'x64'
        })
      ).toThrow(/requires x64/)
    })
  })

  it('fails when a required binary is missing entirely', async () => {
    await withTempDir(async (dir) => {
      await writeFile(join(dir, 'orca_node.node'), elfHeader(0x3e))
      expect(() =>
        assertBundledBinaryArchitectures({
          resourcesDir: dir,
          electronPlatformName: 'linux',
          arch: 'x64'
        })
      ).toThrow(/orca-daemon/)
    })
  })

  it('skips the rust daemon on Windows but still checks the addon', async () => {
    await withTempDir(async (dir) => {
      await writeFile(join(dir, 'orca_node.node'), peHeader(0xaa64)) // arm64 addon
      expect(() =>
        assertBundledBinaryArchitectures({
          resourcesDir: dir,
          electronPlatformName: 'win32',
          arch: 1 // electron-builder Arch.x64
        })
      ).toThrow(/requires x64/)
    })
  })

  it('maps electron-builder numeric Arch values', async () => {
    await withTempDir(async (dir) => {
      await writeFile(join(dir, 'orca-daemon'), elfHeader(0xb7))
      await writeFile(join(dir, 'orca_node.node'), elfHeader(0xb7))
      expect(() =>
        assertBundledBinaryArchitectures({
          resourcesDir: dir,
          electronPlatformName: 'linux',
          arch: 3 // electron-builder Arch.arm64
        })
      ).not.toThrow()
    })
  })

  it('skips silently when no arch is provided (unit-test afterPack calls)', async () => {
    await withTempDir(async (dir) => {
      expect(() =>
        assertBundledBinaryArchitectures({
          resourcesDir: dir,
          electronPlatformName: 'linux',
          arch: undefined
        })
      ).not.toThrow()
    })
  })
})
