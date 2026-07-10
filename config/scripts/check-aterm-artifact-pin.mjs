#!/usr/bin/env node

// Drift guard: the committed renderer wasm artifacts (aterm_wasm_bg.wasm,
// aterm_gpu_web_bg.wasm) are built from the pinned rust/aterm submodule, but nothing
// enforced that the two move together — a `git submodule` bump without a wasm rebuild
// (or vice versa) would ship a renderer engine that disagrees with the pin. Both
// halves embed the engine's `aterm(<version>)` marker. A full build also records
// the exact submodule commit plus SHA-256 and byte length for all eight generated
// files, so same-version commits and stale glue cannot pass this offline gate.
//
// Runs offline (no build). Requires the submodule to be initialized (CI does this);
// exits non-zero with guidance otherwise so it is never silently skipped.
//
// Usage: node config/scripts/check-aterm-artifact-pin.mjs

import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { readFileSync, existsSync } from 'node:fs'
import { resolve } from 'node:path'

const ROOT = resolve(import.meta.dirname, '../..')
const SUBMODULE_MANIFEST = resolve(ROOT, 'rust/aterm/Cargo.toml')
const SUBMODULE = resolve(ROOT, 'rust/aterm')
const WASM_DIR = resolve(ROOT, 'src/renderer/src/lib/pane-manager/aterm')
const ARTIFACT_PIN = resolve(WASM_DIR, 'aterm_wasm_artifact_pin.json')
const WASM_ARTIFACTS = [
  'aterm_wasm.js',
  'aterm_wasm.d.ts',
  'aterm_wasm_bg.wasm',
  'aterm_wasm_bg.wasm.d.ts',
  'aterm_gpu_web.js',
  'aterm_gpu_web.d.ts',
  'aterm_gpu_web_bg.wasm',
  'aterm_gpu_web_bg.wasm.d.ts'
]
const MARKER = /aterm\((\d+\.\d+\.\d+)\)/

function fail(msg) {
  console.error(`[check-aterm-pin] ${msg}`)
  process.exit(1)
}

// The engine version the submodule is pinned to (workspace.package.version).
function submoduleVersion() {
  if (!existsSync(SUBMODULE_MANIFEST)) {
    fail(
      'rust/aterm submodule is not initialized — run `git submodule update --init rust/aterm` first.'
    )
  }
  const toml = readFileSync(SUBMODULE_MANIFEST, 'utf8')
  // The version lives under [workspace.package]; grab the first `version = "x"` after it.
  const section = toml.slice(toml.indexOf('[workspace.package]'))
  const m = section.match(/version\s*=\s*"([^"]+)"/)
  if (!m) {
    fail('could not read [workspace.package] version from rust/aterm/Cargo.toml')
  }
  return m[1]
}

// The engine version embedded in a committed wasm blob's `aterm(x.y.z)` marker.
function artifactVersion(file) {
  const path = resolve(WASM_DIR, file)
  if (!existsSync(path)) {
    fail(`committed artifact missing: ${file}`)
  }
  // latin1 keeps every byte 1:1 so the ASCII marker survives the binary read.
  const m = readFileSync(path).toString('latin1').match(MARKER)
  if (!m) {
    fail(`no aterm(version) marker found in ${file} — is it a stale or corrupt build?`)
  }
  return m[1]
}

function submoduleCommit() {
  try {
    return execFileSync('git', ['-C', SUBMODULE, 'rev-parse', 'HEAD'], {
      encoding: 'utf8'
    }).trim()
  } catch {
    fail('could not read the rust/aterm submodule commit')
  }
}

function submoduleIsClean() {
  try {
    return (
      execFileSync('git', ['-C', SUBMODULE, 'status', '--porcelain'], {
        encoding: 'utf8'
      }).trim().length === 0
    )
  } catch {
    fail('could not inspect the rust/aterm submodule worktree')
  }
}

function artifactPin() {
  if (!existsSync(ARTIFACT_PIN)) {
    fail('exact artifact pin is missing — run `pnpm build:aterm-wasm`.')
  }
  try {
    const pin = JSON.parse(readFileSync(ARTIFACT_PIN, 'utf8'))
    if (pin.schema !== 1 || typeof pin.sourceCommit !== 'string' || !pin.artifacts) {
      fail('exact artifact pin has an unsupported shape — run `pnpm build:aterm-wasm`.')
    }
    return pin
  } catch (error) {
    fail(`could not parse exact artifact pin: ${error instanceof Error ? error.message : error}`)
  }
}

function artifactIdentity(file) {
  const path = resolve(WASM_DIR, file)
  if (!existsSync(path)) {
    fail(`committed artifact missing: ${file}`)
  }
  const bytes = readFileSync(path)
  return {
    bytes: bytes.byteLength,
    sha256: createHash('sha256').update(bytes).digest('hex')
  }
}

const pin = submoduleVersion()
const mismatches = []
for (const file of ['aterm_wasm_bg.wasm', 'aterm_gpu_web_bg.wasm']) {
  const version = artifactVersion(file)
  if (version !== pin) {
    mismatches.push(`${file} is aterm(${version}) but the submodule pin is ${pin}`)
  }
}
const exact = artifactPin()
const commit = submoduleCommit()
if (!submoduleIsClean()) {
  mismatches.push('rust/aterm has uncommitted source changes, so artifact provenance is not exact')
}
if (exact.sourceCommit !== commit) {
  mismatches.push(`artifact manifest pins ${exact.sourceCommit} but rust/aterm is ${commit}`)
}
for (const file of WASM_ARTIFACTS) {
  const expected = exact.artifacts[file]
  const actual = artifactIdentity(file)
  if (!expected || expected.bytes !== actual.bytes || expected.sha256 !== actual.sha256) {
    mismatches.push(`${file} does not match its exact size/SHA-256 pin`)
  }
}

if (mismatches.length > 0) {
  console.error('[check-aterm-pin] committed wasm does not match the aterm submodule pin:')
  for (const m of mismatches) {
    console.error(`  - ${m}`)
  }
  fail(
    'rebuild the renderer wasm to match the pin: `pnpm build:aterm-wasm` (or `pnpm bump:aterm`).'
  )
}

console.log(
  `[check-aterm-pin] ok — 8 committed artifacts match aterm ${pin} at ${commit.slice(0, 12)}.`
)
