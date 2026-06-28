// Build + size-optimize the aterm renderer wasm glue (CPU `aterm-wasm` and GPU
// `aterm-gpu-web`) and copy it into the renderer.
//
// The crates LIVE in aterm (vendored at rust/aterm/crates/*), so this builds
// them from there. Two wrinkles handled here:
//  1. Offline vendor: rust/.cargo/config.toml replaces crates-io with the
//     offline rust/vendor (which intentionally lacks the web deps wgpu-webgl/
//     wasm-bindgen/web-sys). cargo reads config from the INVOCATION CWD, not the
//     manifest — so we invoke from the repo ROOT (no .cargo there) with
//     CARGO_NET_OFFLINE=false to resolve the web deps online.
//  2. wasm-bindgen pin: both crates use =0.2.108; we use a cached CLI under
//     config/.tooling (bootstrapped via cargo install if missing).
//
// Usage: node config/scripts/build-aterm-wasm.mjs [--cpu] [--gpu]  (default: both)
import { execFileSync } from 'node:child_process'
import { existsSync, copyFileSync, statSync, mkdirSync, rmSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..')
const DEST = join(ROOT, 'src/renderer/src/lib/pane-manager/aterm')
const WASM_TARGET_DIR = join(ROOT, 'rust/aterm/target/wasm32-unknown-unknown/release')
const GLUE_OUT = join(ROOT, 'rust/aterm/target/aterm-web-glue')
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

const CRATES = {
  cpu: { dir: 'rust/aterm/crates/aterm-wasm', stem: 'aterm_wasm' },
  gpu: { dir: 'rust/aterm/crates/aterm-gpu-web', stem: 'aterm_gpu_web' }
}

function run(cmd, args, opts = {}) {
  execFileSync(cmd, args, { cwd: ROOT, stdio: 'inherit', ...opts })
}

function which(bin) {
  try {
    execFileSync('sh', ['-c', `command -v ${bin}`], { stdio: 'ignore' })
    return true
  } catch {
    return false
  }
}

function resolveWasmBindgen() {
  const cached = join(WB_DIR, 'bin/wasm-bindgen')
  if (existsSync(cached)) {
    return cached
  }
  // Bootstrap the exact pinned CLI once (cached, gitignored) so the build is
  // reproducible regardless of the system wasm-bindgen version.
  console.log(`[aterm-wasm] bootstrapping wasm-bindgen-cli ${WB_VERSION} → ${WB_DIR}`)
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

function buildCrate(key, wasmBindgen) {
  const { dir, stem } = CRATES[key]
  console.log(`\n[aterm-wasm] building ${key} (${dir}) …`)
  // Build from ROOT (online ancestry) via --manifest-path so the web deps
  // resolve from crates.io, not the offline rust/vendor. Force opt-level="z" for
  // the WHOLE wasm build (engine code is compiled INTO the wasm, so size-opt must
  // cover it, not just the leaf crate) — these are browser download assets. The
  // engine's native profile (opt-3) is unaffected; this only governs this build.
  run(
    'cargo',
    [
      'build',
      '--release',
      '--target',
      'wasm32-unknown-unknown',
      '--manifest-path',
      join(dir, 'Cargo.toml'),
      '--config',
      'profile.release.opt-level="z"'
    ],
    // Pin to stable (aterm's rust-toolchain.toml channel): invoked from ROOT, the
    // machine's global rustup default applies, and an older nightly default may
    // lack the wasm32-unknown-unknown target (or violate aterm's rust-version).
    { env: { ...process.env, CARGO_NET_OFFLINE: 'false', RUSTUP_TOOLCHAIN: 'stable' } }
  )

  const wasm = join(WASM_TARGET_DIR, `${stem}.wasm`)
  const pkg = join(GLUE_OUT, stem)
  rmSync(pkg, { recursive: true, force: true })
  mkdirSync(pkg, { recursive: true })
  run(wasmBindgen, ['--target', 'web', '--out-dir', pkg, wasm])

  const bg = join(pkg, `${stem}_bg.wasm`)
  const before = statSync(bg).size
  run('wasm-opt', ['-Oz', ...WASM_OPT_FEATURES, '-o', bg, bg])
  const after = statSync(bg).size
  console.log(
    `[aterm-wasm] ${stem}_bg.wasm ${before} -> ${after} bytes ` +
      `(-${(((before - after) * 100) / before).toFixed(1)}% via wasm-opt)`
  )

  for (const ext of ['.js', '.d.ts', '_bg.wasm', '_bg.wasm.d.ts']) {
    copyFileSync(join(pkg, `${stem}${ext}`), join(DEST, `${stem}${ext}`))
  }
  console.log(`[aterm-wasm] copied ${stem} glue → src/renderer/.../aterm/`)
}

if (!which('wasm-opt')) {
  console.error('[aterm-wasm] wasm-opt not found — install binaryen (brew install binaryen)')
  process.exit(1)
}
const wasmBindgen = resolveWasmBindgen()
const flags = process.argv.slice(2)
const keys = flags.length ? flags.map((f) => f.replace(/^--/, '')) : ['cpu', 'gpu']
for (const k of keys) {
  if (!CRATES[k]) {
    console.error(`[aterm-wasm] unknown target "${k}" (use --cpu and/or --gpu)`)
    process.exit(1)
  }
  buildCrate(k, wasmBindgen)
}
console.log('\n[aterm-wasm] done.')
