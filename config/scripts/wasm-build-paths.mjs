import { homedir } from 'node:os'
import { join, resolve } from 'node:path'

export function resolveCargoHome(env = process.env, home = homedir()) {
  return resolve(env.CARGO_HOME || join(home, '.cargo'))
}

// Longest-prefix-first so a nested source (e.g. a crate under the repo root) is
// remapped before its parent; drops '/' and de-dups equal roots.
function remapRustflags(pairs) {
  const seen = new Set()
  return pairs
    .map(([source, replacement]) => [resolve(source), replacement])
    .sort(([left], [right]) => right.length - left.length)
    .filter(([source]) => source !== resolve('/') && !seen.has(source) && seen.add(source))
    .map(([source, replacement]) => `--remap-path-prefix=${source}=${replacement}`)
}

export function wasmPathRemapRustflags({ root, atermSource, env = process.env, home = homedir() }) {
  return remapRustflags([
    [resolveCargoHome(env, home), '/cargo'],
    [atermSource, '/aterm'],
    [root, '/orca'],
    [home, '/builder-home']
  ])
}

// Same defense for the in-repo wasm crates (orca-crypto-wasm / orca-git-wasm):
// remap the crate source, repo root, cargo home, and builder home so release
// panic/source strings can't leak the builder's filesystem into the shipped
// desktop app or the relay bundle uploaded to remote hosts.
export function wasmCratePathRemapRustflags({
  root,
  crateSource,
  env = process.env,
  home = homedir()
}) {
  return remapRustflags([
    [resolveCargoHome(env, home), '/cargo'],
    [crateSource, '/crate'],
    [root, '/orca'],
    [home, '/builder-home']
  ])
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
