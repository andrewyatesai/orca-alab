#!/usr/bin/env node

// Offline drift gate for the in-repo wasm crates (orca-crypto-wasm, orca-git-wasm):
// verifies each committed pin against the committed artifacts and the current crate
// source, so a stale/mismatched E2EE crypto wasm or relay git-parser wasm hard-fails
// the build instead of shipping silently. Mirrors check:aterm-pin for the two
// artifacts the aterm submodule guard does not cover. No cargo/network needed.
//
// Usage: node config/scripts/check-orca-wasm-pins.mjs

import { WASM_CRATE_PINS, verifyCratePin } from './wasm-crate-artifact-pin.mjs'

let failed = false
for (const name of Object.keys(WASM_CRATE_PINS)) {
  const descriptor = WASM_CRATE_PINS[name]
  const mismatches = verifyCratePin(name)
  if (mismatches.length > 0) {
    failed = true
    console.error(`[check-wasm-pins] ${descriptor.label}: committed wasm does not match its pin:`)
    for (const m of mismatches) {
      console.error(`  - ${m}`)
    }
    console.error(`  rebuild to match the pin: \`${descriptor.build}\`.`)
  } else {
    console.log(`[check-wasm-pins] ok — ${descriptor.label} artifacts match their pin.`)
  }
}

if (failed) {
  process.exit(1)
}
