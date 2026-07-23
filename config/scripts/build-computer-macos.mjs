import { spawnSync } from 'node:child_process'
import { chmodSync, copyFileSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { resolveMacComputerUseBundleId } from './mac-computer-use-bundle-id.mjs'
import { prepareSwiftPmManifestApiWorkaround } from './swiftpm-manifest-api-workaround.mjs'

const repoRoot = path.resolve(import.meta.dirname, '../..')
const packagePath = path.join(repoRoot, 'native', 'computer-use-macos')
const binaryPath = path.join(packagePath, '.build', 'release', 'orca-computer-use-macos')
const appPath = path.join(packagePath, '.build', 'release', 'Orca Computer Use.app')
const appExecutablePath = path.join(appPath, 'Contents', 'MacOS', 'orca-computer-use-macos')
const appIconPath = path.join(appPath, 'Contents', 'Resources', 'AppIcon.icns')
const bundleId = resolveMacComputerUseBundleId(process.env)
const displayName = 'Orca Computer Use'
// Why: dev and release helpers must not inherit production credentials or a
// developer's keychain identity; the packaged outer app uses the same seal.
const signingIdentity = '-'
const universalTriples = ['arm64-apple-macosx', 'x86_64-apple-macosx']

if (process.platform !== 'darwin') {
  process.exit(0)
}

const swiftPmWorkaround = process.env.SWIFTPM_CUSTOM_LIBS_DIR
  ? null
  : prepareSwiftPmManifestApiWorkaround({
      cacheRoot: path.join(packagePath, '.build', 'orca-swiftpm-manifest-api')
    })
const swiftBuildEnvironment = swiftPmWorkaround
  ? { ...process.env, SWIFTPM_CUSTOM_LIBS_DIR: swiftPmWorkaround.customLibsDir }
  : undefined
if (swiftPmWorkaround) {
  console.log(
    `[build-computer-macos] Using an isolated SwiftPM ManifestAPI because ${swiftPmWorkaround.incompatiblePrivateInterfaces.join(', ')} does not match its public interface.`
  )
}

buildUniversalBinary()
chmodSync(binaryPath, 0o755)
createHelperApp()

function buildUniversalBinary() {
  const builtBinaries = universalTriples.map((triple) => {
    run(
      'swift',
      ['build', '-c', 'release', '--package-path', packagePath, '--triple', triple],
      swiftBuildEnvironment
    )
    return path.join(packagePath, '.build', triple, 'release', 'orca-computer-use-macos')
  })
  mkdirSync(path.dirname(binaryPath), { recursive: true })
  run('lipo', ['-create', ...builtBinaries, '-output', binaryPath])
}

function createHelperApp() {
  rmSync(appPath, { recursive: true, force: true })
  mkdirSync(path.dirname(appExecutablePath), { recursive: true })
  mkdirSync(path.join(appPath, 'Contents', 'Resources'), { recursive: true })
  copyFileSync(binaryPath, appExecutablePath)
  copyFileSync(path.join(repoRoot, 'resources', 'build', 'icon.icns'), appIconPath)
  chmodSync(appExecutablePath, 0o755)
  writeFileSync(path.join(appPath, 'Contents', 'Info.plist'), infoPlist(), 'utf8')
  const signer = spawnSync('codesign', codesignArgs(signingIdentity, appPath), { stdio: 'inherit' })
  if (signer.signal) {
    process.kill(process.pid, signer.signal)
  }
  if (signer.status !== 0) {
    process.exit(signer.status ?? 1)
  }
}

function codesignArgs(identity, targetPath) {
  return ['--force', '--deep', '--sign', identity, targetPath]
}

function run(command, args, env) {
  const result = spawnSync(command, args, { stdio: 'inherit', ...(env ? { env } : {}) })
  if (result.signal) {
    process.kill(process.pid, result.signal)
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1)
  }
}

function infoPlist() {
  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>orca-computer-use-macos</string>
  <key>CFBundleIdentifier</key>
  <string>${escapePlist(bundleId)}</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleIconFile</key>
  <string>AppIcon</string>
  <key>CFBundleName</key>
  <string>${escapePlist(displayName)}</string>
  <key>CFBundleDisplayName</key>
  <string>${escapePlist(displayName)}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>LSMinimumSystemVersion</key>
  <string>14.0</string>
  <key>LSUIElement</key>
  <true/>
  <key>NSAccessibilityUsageDescription</key>
  <string>Orca Computer Use needs Accessibility permission to read and interact with app interfaces when you ask Orca to use apps.</string>
  <key>NSScreenCaptureUsageDescription</key>
  <string>Orca Computer Use needs Screen Recording permission to capture app windows when you ask Orca to inspect your screen.</string>
</dict>
</plist>
`
}

function escapePlist(value) {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&apos;')
}
