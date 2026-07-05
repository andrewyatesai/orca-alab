#!/usr/bin/env node

// Drift guard: the committed renderer wasm artifacts (aterm_wasm_bg.wasm,
// aterm_gpu_web_bg.wasm) are built from the pinned rust/aterm submodule, but nothing
// enforced that the two move together — a `git submodule` bump without a wasm rebuild
// (or vice versa) would ship a renderer engine that disagrees with the pin. Both
// halves embed the engine's `aterm(<version>)` marker and the submodule records the
// same version in its workspace Cargo.toml; this asserts they match.
//
// Runs offline (no build). Requires the submodule to be initialized (CI does this);
// exits non-zero with guidance otherwise so it is never silently skipped.
//
// Usage: node config/scripts/check-aterm-artifact-pin.mjs

import { readFileSync, existsSync } from 'node:fs'
import { resolve } from 'node:path'

const ROOT = resolve(import.meta.dirname, '../..')
const SUBMODULE_MANIFEST = resolve(ROOT, 'rust/aterm/Cargo.toml')
const WASM_DIR = resolve(ROOT, 'src/renderer/src/lib/pane-manager/aterm')
const WASM_ARTIFACTS = ['aterm_wasm_bg.wasm', 'aterm_gpu_web_bg.wasm']
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

const pin = submoduleVersion()
const mismatches = []
for (const file of WASM_ARTIFACTS) {
  const version = artifactVersion(file)
  if (version !== pin) {
    mismatches.push(`${file} is aterm(${version}) but the submodule pin is ${pin}`)
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

console.log(`[check-aterm-pin] ok — committed wasm and submodule pin agree at aterm ${pin}.`)
