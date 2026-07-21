import { homedir } from 'node:os'
import { join, resolve } from 'node:path'

export function resolveCargoHome(env = process.env, home = homedir()) {
  return resolve(env.CARGO_HOME || join(home, '.cargo'))
}

export function wasmPathRemapRustflags({ root, atermSource, env = process.env, home = homedir() }) {
  const remaps = [
    [resolveCargoHome(env, home), '/cargo'],
    [resolve(atermSource), '/aterm'],
    [resolve(root), '/orca'],
    [resolve(home), '/builder-home']
  ]
  const seen = new Set()
  return remaps
    .sort(([left], [right]) => right.length - left.length)
    .filter(([source]) => source !== resolve('/') && !seen.has(source) && seen.add(source))
    .map(([source, replacement]) => `--remap-path-prefix=${source}=${replacement}`)
}

export function localWasmBuildPaths({ root, atermSource, env = process.env, home = homedir() }) {
  return [
    ...new Set([resolveCargoHome(env, home), resolve(atermSource), resolve(root), resolve(home)])
  ]
    .filter((path) => path !== resolve('/'))
    .sort((left, right) => right.length - left.length)
}

export function assertNoEmbeddedLocalBuildPaths(bytes, options) {
  const artifact = Buffer.isBuffer(bytes) ? bytes : Buffer.from(bytes)
  for (const path of localWasmBuildPaths(options)) {
    if (artifact.includes(Buffer.from(path))) {
      throw new Error(`${options.label ?? 'WASM artifact'} embeds a local build path`)
    }
  }
}

export function containsLocalCargoSourcePath(bytes) {
  const text = Buffer.isBuffer(bytes)
    ? bytes.toString('latin1')
    : Buffer.from(bytes).toString('latin1')
  return text
    .split(String.fromCharCode(0))
    .some(
      (embeddedString) =>
        /(?:\/Users\/[^/]+|\/home\/[^/]+)\/\.cargo\/(?:registry|git)\//.test(embeddedString) ||
        /[A-Za-z]:\\Users\\[^\\]+\\\.cargo\\(?:registry|git)\\/.test(embeddedString)
    )
}
