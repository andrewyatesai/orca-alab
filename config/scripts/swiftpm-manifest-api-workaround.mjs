import { createHash, randomUUID } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  writeFileSync
} from 'node:fs'
import path from 'node:path'

const PACKAGE_DESCRIPTION_LIBRARY = 'libPackageDescription.dylib'
const CACHE_MARKER = '.orca-swiftpm-manifest-api.json'

export function resolveSwiftPmManifestApiPath({
  swiftCommand = 'swift',
  spawnSyncImpl = spawnSync
} = {}) {
  const targetInfo = spawnSyncImpl(swiftCommand, ['-print-target-info'], { encoding: 'utf8' })
  if (targetInfo.status !== 0 || typeof targetInfo.stdout !== 'string') {
    return null
  }

  try {
    const parsed = JSON.parse(targetInfo.stdout)
    const runtimeResourcePath = parsed?.paths?.runtimeResourcePath
    return typeof runtimeResourcePath === 'string'
      ? path.join(runtimeResourcePath, 'pm', 'ManifestAPI')
      : null
  } catch {
    return null
  }
}

export function findIncompatiblePrivateInterfaces(manifestApiPath) {
  if (!manifestApiPath || !existsSync(manifestApiPath)) {
    return []
  }

  const incompatible = []
  for (const relativePath of listFilesRecursively(manifestApiPath)) {
    if (!relativePath.endsWith('.private.swiftinterface')) {
      continue
    }
    const publicRelativePath = relativePath.replace('.private.swiftinterface', '.swiftinterface')
    const publicPath = path.join(manifestApiPath, publicRelativePath)
    if (!existsSync(publicPath)) {
      continue
    }

    const privateIdentity = readInterfaceBuildIdentity(path.join(manifestApiPath, relativePath))
    const publicIdentity = readInterfaceBuildIdentity(publicPath)
    if (interfaceBuildIdentitiesConflict(privateIdentity, publicIdentity)) {
      incompatible.push(relativePath)
    }
  }
  return incompatible
}

export function prepareSwiftPmManifestApiWorkaround({
  manifestApiPath = resolveSwiftPmManifestApiPath(),
  cacheRoot
}) {
  const incompatiblePrivateInterfaces = findIncompatiblePrivateInterfaces(manifestApiPath)
  if (incompatiblePrivateInterfaces.length === 0) {
    return null
  }
  if (!cacheRoot) {
    throw new Error('A cache root is required for the SwiftPM ManifestAPI workaround')
  }

  const sourceFiles = compatibleManifestApiFiles(manifestApiPath)
  validateCompatibleManifestApiFiles(sourceFiles)
  const cacheKey = manifestApiCacheKey(manifestApiPath, sourceFiles)
  const customLibsDir = path.join(cacheRoot, cacheKey)
  if (!isCompleteCache(customLibsDir, cacheKey, sourceFiles)) {
    createCompatibleManifestApiCache({
      manifestApiPath,
      sourceFiles,
      customLibsDir,
      cacheKey
    })
  }

  return { customLibsDir, incompatiblePrivateInterfaces }
}

function readInterfaceBuildIdentity(interfacePath) {
  const source = readFileSync(interfacePath, 'utf8')
  const compilerLine = source.match(/^\/\/ swift-compiler-version:\s*(.+)$/m)?.[1] ?? ''
  const compilerBuild = compilerLine.match(/\b(swiftlang-[^\s)]+)/)?.[1]
  const compilerVersion = compilerLine.match(/\bApple Swift version\s+([^\s]+)/)?.[1]
  const moduleVersion = source.match(/\b-user-module-version\s+([^\s]+)/)?.[1]
  return {
    compiler: compilerBuild ?? compilerVersion ?? null,
    moduleVersion: moduleVersion ?? null
  }
}

function interfaceBuildIdentitiesConflict(left, right) {
  const compilerConflict = left.compiler && right.compiler && left.compiler !== right.compiler
  const moduleConflict =
    left.moduleVersion && right.moduleVersion && left.moduleVersion !== right.moduleVersion
  return Boolean(compilerConflict || moduleConflict)
}

function compatibleManifestApiFiles(manifestApiPath) {
  return listFilesRecursively(manifestApiPath).filter(
    (relativePath) =>
      relativePath === PACKAGE_DESCRIPTION_LIBRARY ||
      relativePath.endsWith('.swiftdoc') ||
      (relativePath.endsWith('.swiftinterface') &&
        !relativePath.endsWith('.private.swiftinterface'))
  )
}

function validateCompatibleManifestApiFiles(sourceFiles) {
  if (!sourceFiles.includes(PACKAGE_DESCRIPTION_LIBRARY)) {
    throw new Error(`SwiftPM ManifestAPI is missing ${PACKAGE_DESCRIPTION_LIBRARY}`)
  }
  if (
    !sourceFiles.some(
      (relativePath) =>
        relativePath.startsWith(`PackageDescription.swiftmodule${path.sep}`) &&
        relativePath.endsWith('.swiftinterface')
    )
  ) {
    throw new Error('SwiftPM ManifestAPI is missing a public PackageDescription interface')
  }
}

function manifestApiCacheKey(manifestApiPath, sourceFiles) {
  const hash = createHash('sha256')
  hash.update(manifestApiPath)
  for (const relativePath of sourceFiles) {
    hash.update('\0')
    hash.update(relativePath)
    hash.update('\0')
    hash.update(readFileSync(path.join(manifestApiPath, relativePath)))
  }
  return hash.digest('hex').slice(0, 20)
}

function isCompleteCache(customLibsDir, cacheKey, sourceFiles) {
  try {
    const marker = JSON.parse(readFileSync(path.join(customLibsDir, CACHE_MARKER), 'utf8'))
    return (
      marker.cacheKey === cacheKey &&
      sourceFiles.every((relativePath) =>
        existsSync(path.join(customLibsDir, 'ManifestAPI', relativePath))
      )
    )
  } catch {
    return false
  }
}

function createCompatibleManifestApiCache({
  manifestApiPath,
  sourceFiles,
  customLibsDir,
  cacheKey
}) {
  mkdirSync(path.dirname(customLibsDir), { recursive: true })
  const temporaryDir = `${customLibsDir}.${process.pid}.${randomUUID()}.tmp`
  const targetManifestApiPath = path.join(temporaryDir, 'ManifestAPI')

  try {
    for (const relativePath of sourceFiles) {
      const destinationPath = path.join(targetManifestApiPath, relativePath)
      mkdirSync(path.dirname(destinationPath), { recursive: true })
      copyFileSync(path.join(manifestApiPath, relativePath), destinationPath)
    }
    writeFileSync(
      path.join(temporaryDir, CACHE_MARKER),
      `${JSON.stringify({ cacheKey, sourceFiles }, null, 2)}\n`,
      'utf8'
    )
    try {
      renameSync(temporaryDir, customLibsDir)
    } catch (error) {
      // Another build may have populated the same content-addressed cache while
      // this process was copying. Reuse it only after validating its marker/files.
      if (isCompleteCache(customLibsDir, cacheKey, sourceFiles)) {
        return
      }
      rmSync(customLibsDir, { recursive: true, force: true })
      try {
        renameSync(temporaryDir, customLibsDir)
      } catch {
        if (!isCompleteCache(customLibsDir, cacheKey, sourceFiles)) {
          throw error
        }
      }
    }
  } finally {
    rmSync(temporaryDir, { recursive: true, force: true })
  }
}

function listFilesRecursively(directoryPath, relativeDirectory = '') {
  const files = []
  const currentPath = path.join(directoryPath, relativeDirectory)
  for (const entry of readdirSync(currentPath, { withFileTypes: true })) {
    const relativePath = path.join(relativeDirectory, entry.name)
    if (entry.isDirectory()) {
      files.push(...listFilesRecursively(directoryPath, relativePath))
    } else if (entry.isFile()) {
      files.push(relativePath)
    }
  }
  return files.sort()
}
