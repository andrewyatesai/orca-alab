// Build the orca-crypto wasm binding (the app's E2EE crypto substrate) and copy
// the wasm-bindgen glue into src/shared/crypto-wasm/ (base64-embedded, for the
// Node main/CLI processes via initSync) and src/renderer/src/lib/crypto-wasm/
// (raw _bg.wasm, for the browser via vite `?url` + async init). Every process
// then encrypts through the SAME Rust orca-crypto code, byte-identical to
// tweetnacl, instead of the two hand-maintained TS twins.
//
// Two wrinkles (identical to build-orca-git-wasm.mjs):
//  1. Offline vendor: rust/.cargo/config.toml replaces crates-io with the offline
//     rust/vendor (which lacks wasm-bindgen). cargo reads config from the
//     INVOCATION CWD, so we invoke from the repo ROOT (no vendoring config) with
//     CARGO_NET_OFFLINE=false to resolve wasm-bindgen online. orca-crypto-wasm is
//     its OWN workspace (rust/Cargo.toml excludes it) so this never touches the
//     main offline lock.
//  2. wasm-bindgen pin: =0.2.108 via the cached CLI under config/.tooling.
//
// Crypto is not a throughput hot path (RPC messages are small), so size-optimise
// (opt-level="z" + wasm-opt -Oz).
//
// Usage: node config/scripts/build-orca-crypto-wasm.mjs
import { execFileSync } from 'node:child_process'
import {
  existsSync,
  copyFileSync,
  readFileSync,
  statSync,
  mkdirSync,
  rmSync,
  writeFileSync
} from 'node:fs'
import { join, delimiter } from 'node:path'
import {
  assertNoEmbeddedLocalBuildPaths,
  wasmCratePathRemapRustflags
} from './wasm-build-paths.mjs'
import { RustToolchainError, cargoInvocation, toolchainForTriple } from './rust-toolchain-lane.mjs'
import { writeCratePin } from './wasm-crate-artifact-pin.mjs'

const ROOT = join(import.meta.dirname, '..', '..')
const CRATE_DIR = 'rust/orca-crypto-wasm'
const STEM = 'orca_crypto_wasm'
const DEST = join(ROOT, 'src/shared/crypto-wasm')
const WASM_TARGET_DIR = join(ROOT, CRATE_DIR, 'target/wasm32-unknown-unknown/release')
const GLUE_OUT = join(ROOT, CRATE_DIR, 'target/web-glue')
const WB_VERSION = '0.2.108'
const WB_DIR = join(ROOT, 'config/.tooling', `wasm-bindgen-${WB_VERSION}`)
// wasm-opt rejects the module unless the features it uses are enabled explicitly.
const WASM_OPT_FEATURES = [
  '--enable-bulk-memory',
  '--enable-nontrapping-float-to-int',
  '--enable-sign-ext',
  '--enable-mutable-globals',
  '--enable-reference-types'
]

function run(cmd, args, opts = {}) {
  execFileSync(cmd, args, { cwd: ROOT, stdio: 'inherit', ...opts })
}

function which(bin) {
  // Probe PATH in-process: `sh -c command -v` doesn't exist on Windows.
  const exts =
    process.platform === 'win32' ? (process.env.PATHEXT ?? '.EXE;.CMD;.BAT;.COM').split(';') : ['']
  for (const dir of (process.env.PATH ?? '').split(delimiter)) {
    if (dir && exts.some((ext) => existsSync(join(dir, bin + ext)))) {
      return true
    }
  }
  return false
}

// STOCK EXCEPTION (Trust lacks a wasm32-unknown-unknown std; measured E0463
// "can't find crate for `core`", and -Zbuild-std fails because the seal ships no
// rust-src): while the installed Trust toolchain ships no wasm32 std (probed per
// run in rust-toolchain-lane.mjs), the wasm32 build uses an INSTALLED stock
// rustup toolchain (ORCA_STOCK_RUST_TOOLCHAIN, default `stable`) with cargo and
// rustc pinned by absolute path. Missing rustup/toolchain/target is an error
// naming what to install — never an install, and never a bare `cargo` fallback.
function runWasmCargo(args, opts = {}) {
  const tool = toolchainForTriple({
    triple: 'wasm32-unknown-unknown',
    env: opts.env ?? process.env,
    label: 'orca-crypto-wasm',
    announce: false
  })
  run(tool.command, [...tool.laneArgs, ...args], { ...opts, env: tool.env })
}

function resolveWasmBindgen() {
  const cached = join(WB_DIR, 'bin/wasm-bindgen')
  if (existsSync(cached)) {
    return cached
  }
  console.log(`[orca-crypto-wasm] bootstrapping wasm-bindgen-cli ${WB_VERSION} → ${WB_DIR}`)
  // A HOST tool: targo (`targo --unverified install`) where Trust serves the host.
  const install = cargoInvocation({
    verb: 'install',
    args: ['wasm-bindgen-cli', '--version', WB_VERSION, '--root', WB_DIR, '--locked'],
    label: 'orca-crypto-wasm'
  })
  run(install.command, install.args, { env: install.env })
  return cached
}

let wasmBindgen
try {
  // Fail fast, before the CLI bootstrap, when no installed toolchain can build
  // wasm32 (this also logs the STOCK EXCEPTION and the probe result behind it).
  toolchainForTriple({ triple: 'wasm32-unknown-unknown', label: 'orca-crypto-wasm' })
  wasmBindgen = resolveWasmBindgen()
} catch (error) {
  if (!(error instanceof RustToolchainError)) {
    throw error
  }
  console.error(`[orca-crypto-wasm] ${error.message}`)
  process.exit(1)
}

console.log(`\n[orca-crypto-wasm] building ${CRATE_DIR} …`)
// Build from ROOT (online ancestry) via --manifest-path so wasm-bindgen resolves
// from crates.io, not the offline rust/vendor. runWasmCargo pins the wasm32 lane
// (today the STOCK EXCEPTION; the crate's own rust-toolchain.toml says `stable`
// too, but cargo never reads it from the ROOT cwd).
runWasmCargo(
  [
    'build',
    '--release',
    '--target',
    'wasm32-unknown-unknown',
    '--manifest-path',
    join(CRATE_DIR, 'Cargo.toml')
  ],
  {
    env: {
      ...process.env,
      CARGO_NET_OFFLINE: 'false',
      // Remap builder paths so release panic/source strings can't leak the
      // builder's home/username into the crypto wasm embedded in the desktop app.
      CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS: [
        process.env.CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS,
        ...wasmCratePathRemapRustflags({ root: ROOT, crateSource: join(ROOT, CRATE_DIR) })
      ]
        .filter(Boolean)
        .join(' ')
    }
  }
)

const wasm = join(WASM_TARGET_DIR, `${STEM}.wasm`)
rmSync(GLUE_OUT, { recursive: true, force: true })
mkdirSync(GLUE_OUT, { recursive: true })
run(wasmBindgen, ['--target', 'web', '--out-dir', GLUE_OUT, wasm])

const bg = join(GLUE_OUT, `${STEM}_bg.wasm`)
if (which('wasm-opt')) {
  const before = statSync(bg).size
  run('wasm-opt', ['-Oz', ...WASM_OPT_FEATURES, '-o', bg, bg])
  const after = statSync(bg).size
  console.log(
    `[orca-crypto-wasm] ${STEM}_bg.wasm ${before} -> ${after} bytes ` +
      `(-${(((before - after) * 100) / before).toFixed(1)}% via wasm-opt)`
  )
} else {
  console.warn(
    '[orca-crypto-wasm] wasm-opt not found on PATH — shipping un-optimised wasm (install binaryen to shrink it)'
  )
}

// Hard-fail before embedding if any builder path survived the remap above — the
// _bg.wasm here is the single source for every copy/base64 embed below.
assertNoEmbeddedLocalBuildPaths(readFileSync(bg), {
  root: ROOT,
  atermSource: join(ROOT, CRATE_DIR),
  label: `${STEM}_bg.wasm`
})

// Node processes (main + CLI): base64-embedded module + initSync, so the crypto
// loads identically under electron-vite (main), the CLI bundle, and Node tests
// with no loader config. The raw _bg.wasm is gitignored here (derivable).
mkdirSync(DEST, { recursive: true })
for (const ext of ['.js', '.d.ts', '_bg.wasm', '_bg.wasm.d.ts']) {
  copyFileSync(join(GLUE_OUT, `${STEM}${ext}`), join(DEST, `${STEM}${ext}`))
}
const b64 = readFileSync(join(DEST, `${STEM}_bg.wasm`)).toString('base64')
writeFileSync(
  join(DEST, `${STEM}_bg.wasm.base64.ts`),
  `// GENERATED by config/scripts/build-orca-crypto-wasm.mjs — do not edit.\n` +
    `// base64 of ${STEM}_bg.wasm, embedded so the Node main/CLI bundles stay self-contained.\n` +
    `export const ORCA_CRYPTO_WASM_BASE64 =\n  '${b64}'\n`
)
console.log(
  `[orca-crypto-wasm] wrote glue + ${STEM}_bg.wasm.base64.ts (${b64.length} b64 chars) → src/shared/crypto-wasm/`
)

// The RENDERER loads the same module via vite's `?url` asset + async init (the
// aterm/orca-git precedent — no sync-compile on the Chromium main thread). Its
// copy is committed INCLUDING the raw _bg.wasm so `?url` imports work from a
// fresh checkout.
const RENDERER_DEST = join(ROOT, 'src/renderer/src/lib/crypto-wasm')
mkdirSync(RENDERER_DEST, { recursive: true })
for (const ext of ['.js', '.d.ts', '_bg.wasm', '_bg.wasm.d.ts']) {
  copyFileSync(join(GLUE_OUT, `${STEM}${ext}`), join(RENDERER_DEST, `${STEM}${ext}`))
}
console.log(`[orca-crypto-wasm] copied glue + raw wasm → src/renderer/src/lib/crypto-wasm/`)

// Pin the committed artifacts to the crate source so a source edit without a
// rebuild (or a half-regenerated base64/renderer pair) hard-fails check:wasm-pins.
const pinPath = writeCratePin('crypto')
console.log(`[orca-crypto-wasm] wrote ${pinPath}`)
console.log('\n[orca-crypto-wasm] done.')
