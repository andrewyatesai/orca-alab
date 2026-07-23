import { join } from 'node:path'

function rustHostExecutableName(name, platform) {
  // Cargo installs Windows host binaries with .exe, so probes must use the emitted filename.
  return platform === 'win32' ? `${name}.exe` : name
}

export function cachedWasmBindgenExecutablePath(cacheRoot, platform = process.platform) {
  return join(cacheRoot, 'bin', rustHostExecutableName('wasm-bindgen', platform))
}

export function orcaParityExecutablePaths(projectDir, platform = process.platform) {
  return ['debug', 'release'].map((profile) =>
    join(projectDir, 'rust', 'target', profile, rustHostExecutableName('orca-parity', platform))
  )
}
