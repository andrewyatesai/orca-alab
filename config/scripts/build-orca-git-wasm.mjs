// Build the orca-git wasm binding (the addon-less SSH relay's git parser) and
// copy the wasm-bindgen glue into src/relay/wasm/ (committed, like the aterm
// renderer glue). The relay bundles it via esbuild's binary loader and drives it
// with initSync — so the remote host parses git output through the SAME Rust code
// the main process runs via napi, not a hand-maintained TS reimplementation.
//
// Two wrinkles (identical to build-aterm-wasm.mjs):
//  1. Offline vendor: rust/.cargo/config.toml replaces crates-io with the offline
//     rust/vendor (which lacks wasm-bindgen). cargo reads config from the
//     INVOCATION CWD, so we invoke from the repo ROOT (no .cargo there) with
//     CARGO_NET_OFFLINE=false to resolve wasm-bindgen online. orca-git-wasm is its
//     OWN workspace (rust/Cargo.toml excludes it) so this never touches the main
//     offline lock.
//  2. wasm-bindgen pin: =0.2.108 via the cached CLI under config/.tooling.
//
// Unlike aterm (speed-critical render loop -> -O3), these parsers are not on a
// throughput hot path, so we size-optimise (opt-level="z" + wasm-opt -Oz): the
// artifact ships in the relay bundle uploaded to every remote host.
//
// Usage: node config/scripts/build-orca-git-wasm.mjs
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
import { writeCratePin } from './wasm-crate-artifact-pin.mjs'

const ROOT = join(import.meta.dirname, '..', '..')
const CRATE_DIR = 'rust/orca-git-wasm'
const STEM = 'orca_git_wasm'
const DEST = join(ROOT, 'src/relay/wasm')
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

// Build cargo with the rustup-managed STABLE toolchain explicitly (see
// build-aterm-wasm.mjs for the two PATH-shadow gotchas this defeats). Falls back
// to plain cargo (+ RUSTUP_TOOLCHAIN) when rustup is absent.
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
  console.log(`[orca-git-wasm] bootstrapping wasm-bindgen-cli ${WB_VERSION} → ${WB_DIR}`)
  run('cargo', [
    'install',
    'wasm-bindgen-cli',
    '--version',
    WB_VERSION,
    '--root',
    WB_DIR,
    '--locked'
  ])
  return cached
}

const wasmBindgen = resolveWasmBindgen()

console.log(`\n[orca-git-wasm] building ${CRATE_DIR} …`)
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
  {
    env: {
      ...process.env,
      CARGO_NET_OFFLINE: 'false',
      RUSTUP_TOOLCHAIN: 'stable',
      // Remap builder paths so release panic/source strings can't leak the
      // builder's home/username into the git wasm shipped in the relay bundle
      // uploaded to every remote host.
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
  // -Oz (size): these parsers ship to remote hosts and are not throughput-hot.
  run('wasm-opt', ['-Oz', ...WASM_OPT_FEATURES, '-o', bg, bg])
  const after = statSync(bg).size
  console.log(
    `[orca-git-wasm] ${STEM}_bg.wasm ${before} -> ${after} bytes ` +
      `(-${(((before - after) * 100) / before).toFixed(1)}% via wasm-opt)`
  )
} else {
  // Optional: the wasm is correct un-opted, just larger. Don't hard-fail a build
  // on a dev box without binaryen (the relay artifact is only shipped on release).
  console.warn(
    '[orca-git-wasm] wasm-opt not found on PATH — shipping un-optimised wasm (install binaryen to shrink it)'
  )
}

// Hard-fail before embedding if any builder path survived the remap above — the
// _bg.wasm here is the single source for the relay base64 embed and renderer copy.
assertNoEmbeddedLocalBuildPaths(readFileSync(bg), {
  root: ROOT,
  atermSource: join(ROOT, CRATE_DIR),
  label: `${STEM}_bg.wasm`
})

mkdirSync(DEST, { recursive: true })
for (const ext of ['.js', '.d.ts', '_bg.wasm', '_bg.wasm.d.ts']) {
  copyFileSync(join(GLUE_OUT, `${STEM}${ext}`), join(DEST, `${STEM}${ext}`))
}

// The RENDERER loads the same module via vite's `?url` asset + async init (the
// aterm precedent — no sync-compile on the Chromium main thread, no base64
// bundle bloat). Its copy is committed INCLUDING the raw _bg.wasm (unlike the
// relay dir, where the raw wasm is gitignored in favour of the base64 embed)
// so `?url` imports work from a fresh checkout.
const RENDERER_DEST = join(ROOT, 'src/renderer/src/lib/git-wasm')
mkdirSync(RENDERER_DEST, { recursive: true })
for (const ext of ['.js', '.d.ts', '_bg.wasm', '_bg.wasm.d.ts']) {
  copyFileSync(join(GLUE_OUT, `${STEM}${ext}`), join(RENDERER_DEST, `${STEM}${ext}`))
}
console.log(`[orca-git-wasm] copied glue + raw wasm → src/renderer/src/lib/git-wasm/`)

// Embed the wasm bytes as a base64 TS module so the relay stays a SINGLE
// self-contained relay.js. A raw-file + readFileSync/import.meta.url approach
// resolves differently under vite (relay tests), esbuild (the bundle), and the
// remote Node runtime; a base64 string import is byte-identical across all three
// and needs no bundler loader config. The raw _bg.wasm is gitignored (derivable).
const b64 = readFileSync(join(DEST, `${STEM}_bg.wasm`)).toString('base64')
writeFileSync(
  join(DEST, `${STEM}_bg.wasm.base64.ts`),
  `// GENERATED by config/scripts/build-orca-git-wasm.mjs — do not edit.\n` +
    `// base64 of ${STEM}_bg.wasm, embedded so the relay bundle stays self-contained.\n` +
    `export const ORCA_GIT_WASM_BASE64 =\n  '${b64}'\n`
)
console.log(
  `[orca-git-wasm] copied glue + wrote ${STEM}_bg.wasm.base64.ts (${b64.length} b64 chars) → src/relay/wasm/`
)

// Pin the committed artifacts to the crate source so a source edit without a
// rebuild (or a half-regenerated base64/renderer pair) hard-fails check:wasm-pins.
const pinPath = writeCratePin('git')
console.log(`[orca-git-wasm] wrote ${pinPath}`)
console.log('\n[orca-git-wasm] done.')
