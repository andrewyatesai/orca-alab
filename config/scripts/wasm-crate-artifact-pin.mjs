// Drift guard for the in-repo wasm crates orca-crypto-wasm (the app's E2EE crypto
// substrate) and orca-git-wasm (the git parser bundled into the relay uploaded to
// every remote host). Unlike aterm — a git SUBMODULE whose source moves without a
// visible diff, hence check-aterm-artifact-pin.mjs — these crates are in-tree, so
// their Rust source is reviewed in the same diff as the artifact. What still drifts
// silently is a source edit with no `pnpm build:crypto-wasm`/`build:relay-wasm`
// rebuild, or the two byte-encodings of one build (the base64 module vs the raw
// renderer wasm) getting out of step. This module records, per crate: a SHA-256 of
// the crate source tree, a SHA-256 + byte length of every committed glue artifact,
// and the shared wasm SHA-256 — and verifies them offline (no cargo/network).
import { createHash } from 'node:crypto'
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { join, resolve, sep } from 'node:path'

const ROOT = resolve(import.meta.dirname, '../..')

// One descriptor per crate. `base64Module` ships in the Node/CLI (crypto) or relay
// (git) bundle; `rawWasm` is the committed renderer copy. They are two encodings of
// ONE build output and MUST decode byte-identical — that cross-check is what catches
// a half-regenerated pair. Paths are repo-relative so pins/findings stay portable.
export const WASM_CRATE_PINS = {
  crypto: {
    label: 'orca-crypto-wasm',
    build: 'pnpm build:crypto-wasm',
    sourceDir: 'rust/orca-crypto-wasm',
    pinPath: 'src/shared/crypto-wasm/orca_crypto_wasm_artifact_pin.json',
    base64Module: 'src/shared/crypto-wasm/orca_crypto_wasm_bg.wasm.base64.ts',
    rawWasm: 'src/renderer/src/lib/crypto-wasm/orca_crypto_wasm_bg.wasm',
    artifacts: [
      'src/shared/crypto-wasm/orca_crypto_wasm.js',
      'src/shared/crypto-wasm/orca_crypto_wasm.d.ts',
      'src/shared/crypto-wasm/orca_crypto_wasm_bg.wasm.base64.ts',
      'src/shared/crypto-wasm/orca_crypto_wasm_bg.wasm.d.ts',
      'src/renderer/src/lib/crypto-wasm/orca_crypto_wasm.js',
      'src/renderer/src/lib/crypto-wasm/orca_crypto_wasm.d.ts',
      'src/renderer/src/lib/crypto-wasm/orca_crypto_wasm_bg.wasm',
      'src/renderer/src/lib/crypto-wasm/orca_crypto_wasm_bg.wasm.d.ts'
    ]
  },
  git: {
    label: 'orca-git-wasm',
    build: 'pnpm build:relay-wasm',
    sourceDir: 'rust/orca-git-wasm',
    pinPath: 'src/relay/wasm/orca_git_wasm_artifact_pin.json',
    base64Module: 'src/relay/wasm/orca_git_wasm_bg.wasm.base64.ts',
    rawWasm: 'src/renderer/src/lib/git-wasm/orca_git_wasm_bg.wasm',
    artifacts: [
      'src/relay/wasm/orca_git_wasm.js',
      'src/relay/wasm/orca_git_wasm.d.ts',
      'src/relay/wasm/orca_git_wasm_bg.wasm.base64.ts',
      'src/relay/wasm/orca_git_wasm_bg.wasm.d.ts',
      'src/renderer/src/lib/git-wasm/orca_git_wasm.js',
      'src/renderer/src/lib/git-wasm/orca_git_wasm.d.ts',
      'src/renderer/src/lib/git-wasm/orca_git_wasm_bg.wasm',
      'src/renderer/src/lib/git-wasm/orca_git_wasm_bg.wasm.d.ts'
    ]
  }
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

// Deterministic list of a crate's source files (sorted, `target/` excluded).
function listSourceFiles(absDir, acc = []) {
  for (const name of readdirSync(absDir).sort()) {
    if (name === 'target') {
      continue
    }
    const child = join(absDir, name)
    if (statSync(child).isDirectory()) {
      listSourceFiles(child, acc)
    } else {
      acc.push(child)
    }
  }
  return acc
}

// Hash of the crate source tree: every file's repo-relative-within-crate path and
// bytes, folded in sorted order. Path separators are normalized to '/' so the pin
// is identical on Windows and POSIX.
export function crateSourceSha256(sourceDirRel) {
  const absDir = resolve(ROOT, sourceDirRel)
  const hash = createHash('sha256')
  for (const file of listSourceFiles(absDir)) {
    const rel = file
      .slice(absDir.length + 1)
      .split(sep)
      .join('/')
    hash.update(`${rel}\0`)
    hash.update(readFileSync(file))
    hash.update('\0')
  }
  return hash.digest('hex')
}

// Extract the raw bytes from a `_bg.wasm.base64.ts` module (a single quoted literal).
export function decodeBase64Module(text) {
  const m = text.match(/'([A-Za-z0-9+/=]+)'/)
  if (!m) {
    throw new Error('no base64 literal found in generated module')
  }
  return Buffer.from(m[1], 'base64')
}

function artifactIdentity(absPath) {
  const bytes = readFileSync(absPath)
  return { bytes: bytes.byteLength, sha256: sha256(bytes) }
}

export function buildCratePin(name) {
  const descriptor = WASM_CRATE_PINS[name]
  const artifacts = {}
  for (const rel of descriptor.artifacts) {
    artifacts[rel] = artifactIdentity(resolve(ROOT, rel))
  }
  return {
    schema: 1,
    crate: descriptor.label,
    sourceSha256: crateSourceSha256(descriptor.sourceDir),
    wasmSha256: sha256(readFileSync(resolve(ROOT, descriptor.rawWasm))),
    artifacts
  }
}

export function writeCratePin(name) {
  const descriptor = WASM_CRATE_PINS[name]
  writeFileSync(
    resolve(ROOT, descriptor.pinPath),
    `${JSON.stringify(buildCratePin(name), null, 2)}\n`
  )
  return descriptor.pinPath
}

// Offline verification — reads the committed pin file and returns a list of
// human-readable mismatches (empty = ok).
export function verifyCratePin(name) {
  const descriptor = WASM_CRATE_PINS[name]
  const pinAbs = resolve(ROOT, descriptor.pinPath)
  if (!existsSync(pinAbs)) {
    return [`${descriptor.label} pin is missing — run \`${descriptor.build}\`.`]
  }
  let pin
  try {
    pin = JSON.parse(readFileSync(pinAbs, 'utf8'))
  } catch (error) {
    return [
      `${descriptor.label} pin is unparseable: ${error instanceof Error ? error.message : error}`
    ]
  }
  return diffCratePin(name, pin)
}

// Diff a pin object against the on-disk artifacts + crate source (empty = match).
export function diffCratePin(name, pin) {
  const descriptor = WASM_CRATE_PINS[name]
  if (!pin || pin.schema !== 1 || !pin.artifacts || typeof pin.sourceSha256 !== 'string') {
    return [`${descriptor.label} pin has an unsupported shape — run \`${descriptor.build}\`.`]
  }

  const mismatches = []
  for (const rel of descriptor.artifacts) {
    const abs = resolve(ROOT, rel)
    if (!existsSync(abs)) {
      mismatches.push(`committed artifact missing: ${rel}`)
      continue
    }
    const expected = pin.artifacts[rel]
    const actual = artifactIdentity(abs)
    if (!expected || expected.bytes !== actual.bytes || expected.sha256 !== actual.sha256) {
      mismatches.push(`${rel} does not match its size/SHA-256 pin`)
    }
  }

  const sourceSha256 = crateSourceSha256(descriptor.sourceDir)
  if (sourceSha256 !== pin.sourceSha256) {
    mismatches.push(
      `${descriptor.sourceDir} source changed since the artifacts were built — run \`${descriptor.build}\`.`
    )
  }

  // Cross-check: the shipped base64 module and the renderer raw wasm are one build.
  const rawAbs = resolve(ROOT, descriptor.rawWasm)
  const base64Abs = resolve(ROOT, descriptor.base64Module)
  if (existsSync(rawAbs) && existsSync(base64Abs)) {
    const raw = readFileSync(rawAbs)
    if (sha256(raw) !== pin.wasmSha256) {
      mismatches.push(`${descriptor.rawWasm} does not match the pinned wasm SHA-256`)
    }
    let decoded
    try {
      decoded = decodeBase64Module(readFileSync(base64Abs, 'utf8'))
    } catch (error) {
      mismatches.push(
        `${descriptor.base64Module}: ${error instanceof Error ? error.message : error}`
      )
    }
    if (decoded && Buffer.compare(decoded, raw) !== 0) {
      mismatches.push(
        `${descriptor.base64Module} decodes to bytes that differ from ${descriptor.rawWasm}`
      )
    }
  }
  return mismatches
}
