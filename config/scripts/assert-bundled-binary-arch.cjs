// Verifies that the cargo-built binaries bundled as extraResources
// (orca-daemon, orca_node.node) actually match the CPU architecture of the app
// bundle being packaged. electron-builder only checks that extraResources
// exist — a host-arch binary copied into a foreign-arch DMG passes silently
// and can never exec/dlopen on the target machine (staging-launch audit F2).
//
// Detection is host-tool-light on purpose: Mach-O slices come from `lipo`
// (always present on the macOS hosts that package mac bundles); ELF and PE
// headers are parsed directly so Linux/Windows packaging needs no extra tools.

const { execFileSync } = require('node:child_process')
const { closeSync, existsSync, openSync, readSync } = require('node:fs')
const { join } = require('node:path')

// electron-builder Arch enum ordering (builder-util Arch).
const ELECTRON_BUILDER_ARCH_NAMES = ['ia32', 'x64', 'armv7l', 'arm64', 'universal']

const ELF_MACHINE_ARCH_NAMES = {
  0x03: 'ia32',
  0x28: 'armv7l',
  0x3e: 'x64',
  0xb7: 'arm64'
}

const PE_MACHINE_ARCH_NAMES = {
  0x014c: 'ia32',
  0x01c4: 'armv7l',
  0x8664: 'x64',
  0xaa64: 'arm64'
}

function archNameFromElectronBuilderArch(arch) {
  if (typeof arch === 'string') {
    return arch
  }
  if (typeof arch === 'number') {
    return ELECTRON_BUILDER_ARCH_NAMES[arch] ?? null
  }
  return null
}

function readFileHeader(filePath, length) {
  const fd = openSync(filePath, 'r')
  try {
    const buffer = Buffer.alloc(length)
    const bytesRead = readSync(fd, buffer, 0, length, 0)
    return buffer.subarray(0, bytesRead)
  } finally {
    closeSync(fd)
  }
}

function readElfArchName(filePath) {
  const header = readFileHeader(filePath, 20)
  if (header.length < 20 || header.readUInt32BE(0) !== 0x7f454c46) {
    return null
  }
  // e_machine is a u16 at offset 18 whose byte order follows EI_DATA (offset 5).
  const machine = header[5] === 2 ? header.readUInt16BE(18) : header.readUInt16LE(18)
  return ELF_MACHINE_ARCH_NAMES[machine] ?? null
}

function readPeArchName(filePath) {
  const header = readFileHeader(filePath, 0x40)
  if (header.length < 0x40 || header.readUInt16LE(0) !== 0x5a4d) {
    return null
  }
  const peOffset = header.readUInt32LE(0x3c)
  const peHeader = readFileHeader(filePath, peOffset + 6)
  if (peHeader.length < peOffset + 6 || peHeader.readUInt32LE(peOffset) !== 0x00004550) {
    return null
  }
  return PE_MACHINE_ARCH_NAMES[peHeader.readUInt16LE(peOffset + 4)] ?? null
}

function readMachOArchNames(filePath) {
  const output = execFileSync('lipo', ['-archs', filePath], { encoding: 'utf8' })
  return output
    .trim()
    .split(/\s+/)
    .filter(Boolean)
    .map((token) => {
      if (token === 'x86_64' || token === 'x86_64h') {
        return 'x64'
      }
      // arm64e binaries run wherever plain arm64 does.
      if (token === 'arm64' || token === 'arm64e') {
        return 'arm64'
      }
      if (token === 'i386') {
        return 'ia32'
      }
      return token
    })
}

function readBinaryArchNames(filePath, electronPlatformName) {
  if (electronPlatformName === 'darwin') {
    return readMachOArchNames(filePath)
  }
  if (electronPlatformName === 'win32') {
    const arch = readPeArchName(filePath)
    return arch ? [arch] : []
  }
  const arch = readElfArchName(filePath)
  return arch ? [arch] : []
}

function bundledCargoBinaryNames(electronPlatformName) {
  // Windows ships no Rust daemon (Unix-socket transport); the terminal addon
  // ships everywhere.
  return electronPlatformName === 'win32' ? ['orca_node.node'] : ['orca-daemon', 'orca_node.node']
}

function assertBundledBinaryArchitectures({ resourcesDir, electronPlatformName, arch }) {
  const bundleArchName = archNameFromElectronBuilderArch(arch)
  if (!bundleArchName) {
    // Why: unit tests drive afterPack without an arch; real electron-builder
    // invocations always provide one. Skipping beats guessing wrong.
    return
  }
  const requiredArchNames = bundleArchName === 'universal' ? ['x64', 'arm64'] : [bundleArchName]

  for (const binaryName of bundledCargoBinaryNames(electronPlatformName)) {
    const binaryPath = join(resourcesDir, binaryName)
    if (!existsSync(binaryPath)) {
      throw new Error(
        `[bundle-arch] required binary missing from packaged resources: ${binaryPath}`
      )
    }
    const actualArchNames = readBinaryArchNames(binaryPath, electronPlatformName)
    for (const requiredArchName of requiredArchNames) {
      if (!actualArchNames.includes(requiredArchName)) {
        throw new Error(
          `[bundle-arch] ${binaryPath} contains [${actualArchNames.join(', ') || 'unknown'}] ` +
            `but this ${bundleArchName} bundle requires ${requiredArchName}. ` +
            'Rebuild the cargo artifacts for every packaged arch ' +
            '(ORCA_MAC_RELEASE=1 or ORCA_MAC_BUILD_ARCHES=x64,arm64 for ' +
            'build:terminal-addon and build:rust-daemon) before packaging.'
        )
      }
    }
  }
}

module.exports = {
  assertBundledBinaryArchitectures,
  readElfArchName,
  readMachOArchNames,
  readPeArchName
}
