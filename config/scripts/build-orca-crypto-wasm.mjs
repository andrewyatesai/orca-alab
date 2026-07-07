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
//     INVOCATION CWD, so we invoke from the repo ROOT (no .cargo there) with
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

// Absolute path to a rustup-managed STABLE tool (Homebrew's cargo/rustc on PATH
// shadow rustup and lack the wasm32 target).
function rustupStableBin(bin) {
  return execFileSync('rustup', ['which', bin, '--toolchain', 'stable'], {
    encoding: 'utf8'
  }).trim()
}

// Build cargo with the rustup-managed STABLE toolchain explicitly. Falls back to
// plain cargo (+ RUSTUP_TOOLCHAIN) when rustup is absent.
function runWasmCargo(args, opts = {}) {
  const baseEnv = opts.env ?? process.env
  if (which('rustup')) {
    const cargo = rustupStableBin('cargo')
    const rustc = rustupStableBin('rustc')
    run(cargo, args, { ...opts, env: { ...baseEnv, RUSTC: rustc } })
  } else {
    run('cargo', args, opts)
  }
}

function resolveWasmBindgen() {
  const cached = join(WB_DIR, 'bin/wasm-bindgen')
  if (existsSync(cached)) {
    return cached
  }
  console.log(`[orca-crypto-wasm] bootstrapping wasm-bindgen-cli ${WB_VERSION} → ${WB_DIR}`)
  run('cargo', ['install', 'wasm-bindgen-cli', '--version', WB_VERSION, '--root', WB_DIR, '--locked'])
  return cached
}

const wasmBindgen = resolveWasmBindgen()

console.log(`\n[orca-crypto-wasm] building ${CRATE_DIR} …`)
// Build from ROOT (online ancestry) via --manifest-path so wasm-bindgen resolves
// from crates.io, not the offline rust/vendor. RUSTUP_TOOLCHAIN pins stable for
// the no-rustup fallback (matching rust-toolchain.toml in the crate).
runWasmCargo(
  [
    'build',
    '--release',
    '--target',
    'wasm32-unknown-unknown',
    '--manifest-path',
    join(CRATE_DIR, 'Cargo.toml')
  ],
  { env: { ...process.env, CARGO_NET_OFFLINE: 'false', RUSTUP_TOOLCHAIN: 'stable' } }
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
console.log('\n[orca-crypto-wasm] done.')
