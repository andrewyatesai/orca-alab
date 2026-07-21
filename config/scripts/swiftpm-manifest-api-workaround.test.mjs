import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  findIncompatiblePrivateInterfaces,
  prepareSwiftPmManifestApiWorkaround,
  resolveSwiftPmManifestApiPath
} from './swiftpm-manifest-api-workaround.mjs'

const temporaryRoots = []

afterEach(() => {
  for (const root of temporaryRoots.splice(0)) {
    rmSync(root, { recursive: true, force: true })
  }
})

function makeManifestApiFixture({ privateCompiler = 'swiftlang-5.10.0.13' } = {}) {
  const root = mkdtempSync(path.join(tmpdir(), 'orca-swiftpm-manifest-api-'))
  temporaryRoots.push(root)
  const manifestApiPath = path.join(root, 'toolchain', 'pm', 'ManifestAPI')
  const packageModulePath = path.join(manifestApiPath, 'PackageDescription.swiftmodule')
  const pluginModulePath = path.join(manifestApiPath, 'CompilerPluginSupport.swiftmodule')
  mkdirSync(packageModulePath, { recursive: true })
  mkdirSync(pluginModulePath, { recursive: true })

  writeFileSync(path.join(manifestApiPath, 'libPackageDescription.dylib'), 'dylib fixture')
  writeInterface(
    path.join(packageModulePath, 'arm64-apple-macos.swiftinterface'),
    'swiftlang-6.3.3.1.3',
    '24907'
  )
  writeInterface(
    path.join(packageModulePath, 'arm64-apple-macos.private.swiftinterface'),
    privateCompiler,
    privateCompiler === 'swiftlang-6.3.3.1.3' ? '24907' : '22656'
  )
  writeFileSync(path.join(packageModulePath, 'arm64-apple-macos.swiftdoc'), 'package docs')
  writeInterface(
    path.join(pluginModulePath, 'arm64-apple-macos.swiftinterface'),
    'swiftlang-6.3.3.1.3',
    '24907'
  )
  writeFileSync(path.join(pluginModulePath, 'arm64-apple-macos.swiftdoc'), 'plugin docs')
  writeFileSync(path.join(manifestApiPath, 'unrelated-toolchain-file'), 'must not be copied')
  return { root, manifestApiPath }
}

function writeInterface(filePath, compiler, moduleVersion) {
  writeFileSync(
    filePath,
    `// swift-interface-format-version: 1.0
// swift-compiler-version: Apple Swift version 6.3.3 (${compiler} clang-fixture)
// swift-module-flags: -user-module-version ${moduleVersion} -module-name PackageDescription
`
  )
}

describe('SwiftPM ManifestAPI workaround', () => {
  it('resolves ManifestAPI beside the active Swift runtime resources', () => {
    const spawnSyncImpl = vi.fn(() => ({
      status: 0,
      stdout: JSON.stringify({ paths: { runtimeResourcePath: '/toolchain/usr/lib/swift' } })
    }))

    expect(resolveSwiftPmManifestApiPath({ spawnSyncImpl })).toBe(
      path.join('/toolchain/usr/lib/swift', 'pm', 'ManifestAPI')
    )
    expect(spawnSyncImpl).toHaveBeenCalledWith('swift', ['-print-target-info'], {
      encoding: 'utf8'
    })
  })

  it('does nothing when private and public interfaces come from the same build', () => {
    const { root, manifestApiPath } = makeManifestApiFixture({
      privateCompiler: 'swiftlang-6.3.3.1.3'
    })
    const cacheRoot = path.join(root, 'cache')

    expect(findIncompatiblePrivateInterfaces(manifestApiPath)).toEqual([])
    expect(prepareSwiftPmManifestApiWorkaround({ manifestApiPath, cacheRoot })).toBeNull()
    expect(existsSync(cacheRoot)).toBe(false)
  })

  it('caches only the public ManifestAPI when a private interface is stale', () => {
    const { root, manifestApiPath } = makeManifestApiFixture()
    const cacheRoot = path.join(root, 'cache')
    const staleInterface = path.join(
      'PackageDescription.swiftmodule',
      'arm64-apple-macos.private.swiftinterface'
    )

    expect(findIncompatiblePrivateInterfaces(manifestApiPath)).toEqual([staleInterface])
    const prepared = prepareSwiftPmManifestApiWorkaround({ manifestApiPath, cacheRoot })
    expect(prepared).not.toBeNull()
    if (!prepared) {
      throw new Error('Expected the stale private interface workaround')
    }

    const cachedManifestApi = path.join(prepared.customLibsDir, 'ManifestAPI')
    expect(prepared.incompatiblePrivateInterfaces).toEqual([staleInterface])
    expect(readFileSync(path.join(cachedManifestApi, 'libPackageDescription.dylib'), 'utf8')).toBe(
      'dylib fixture'
    )
    expect(
      existsSync(
        path.join(
          cachedManifestApi,
          'PackageDescription.swiftmodule',
          'arm64-apple-macos.swiftinterface'
        )
      )
    ).toBe(true)
    expect(existsSync(path.join(cachedManifestApi, staleInterface))).toBe(false)
    expect(existsSync(path.join(cachedManifestApi, 'unrelated-toolchain-file'))).toBe(false)

    const cachedPublicInterface = path.join(
      cachedManifestApi,
      'PackageDescription.swiftmodule',
      'arm64-apple-macos.swiftinterface'
    )
    rmSync(cachedPublicInterface)
    const repaired = prepareSwiftPmManifestApiWorkaround({ manifestApiPath, cacheRoot })
    expect(repaired?.customLibsDir).toBe(prepared.customLibsDir)
    expect(existsSync(cachedPublicInterface)).toBe(true)
    expect(existsSync(path.join(manifestApiPath, staleInterface))).toBe(true)
  })
})
