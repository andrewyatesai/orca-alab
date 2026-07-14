// Build + speed-optimize the aterm renderer wasm glue (CPU `aterm-wasm` and GPU
// `aterm-gpu-web`) and copy it into the renderer. Speed over size: the hot render
// loop runs in this wasm every frame, so it inherits aterm's native opt-3 profile.
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
import { createHash } from 'node:crypto'
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
const DEST = join(ROOT, 'src/renderer/src/lib/pane-manager/aterm')
const WASM_TARGET_DIR = join(ROOT, 'rust/aterm/target/wasm32-unknown-unknown/release')
const GLUE_OUT = join(ROOT, 'rust/aterm/target/aterm-web-glue')
const WB_VERSION = '0.2.108'
const WB_DIR = join(ROOT, 'config/.tooling', `wasm-bindgen-${WB_VERSION}`)
const ARTIFACT_PIN = 'aterm_wasm_artifact_pin.json'
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
const ARTIFACTS = Object.values(CRATES).flatMap(({ stem }) => [
  `${stem}.js`,
  `${stem}.d.ts`,
  `${stem}_bg.wasm`,
  `${stem}_bg.wasm.d.ts`
])

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

// Which rustup toolchain builds the wasm. Defaults to STABLE (the proven,
// wasm32-capable path); ORCA_RUST_TOOLCHAIN=trust rebuilds with the Trust-verified
// compiler (`pnpm bump:aterm` honors it too).
const RUST_TOOLCHAIN = process.env.ORCA_RUST_TOOLCHAIN || 'stable'

// Absolute path to a rustup-managed tool (Homebrew's cargo/rustc on PATH shadow
// rustup and lack the wasm32 target).
function rustupStableBin(bin) {
  return execFileSync('rustup', ['which', bin, '--toolchain', RUST_TOOLCHAIN], {
    encoding: 'utf8'
  }).trim()
}

// Build cargo with the rustup-managed STABLE toolchain explicitly. Two shadows to beat:
// (1) a Homebrew cargo on PATH ignores RUSTUP_TOOLCHAIN, and (2) even rustup's stable
// cargo spawns a BARE `rustc` resolved from PATH (Homebrew's 1.95, no wasm32) unless
// RUSTC is pinned. So we invoke stable's cargo by absolute path WITH RUSTC pinned to
// stable's rustc. Falls back to plain cargo (+ RUSTUP_TOOLCHAIN) when rustup is absent.
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
  // resolve from crates.io, not the offline rust/vendor. Inherit aterm's native
  // [profile.release] as-is: opt-level=3 + fat LTO. The per-frame render, glyph
  // layout, and ALL effects run in this wasm every frame (even on the WebGL2 GPU
  // path), so hot-loop speed drives animation smoothness — a size-opt override
  // (opt-level="z") made the wasm visibly chunkier than the native opt-3 build.
  // A few MB more download is a non-issue; smoothness is the product.
  runWasmCargo(
    [
      'build',
      '--release',
      '--target',
      'wasm32-unknown-unknown',
      '--manifest-path',
      join(dir, 'Cargo.toml')
    ],
    // Pin to stable (aterm's rust-toolchain.toml channel): the machine's global rustup
    // default may be an older nightly that lacks the wasm32-unknown-unknown target (or
    // violates aterm's rust-version). RUSTUP_TOOLCHAIN is a belt-and-suspenders for the
    // no-rustup fallback path.
    { env: { ...process.env, CARGO_NET_OFFLINE: 'false', RUSTUP_TOOLCHAIN: 'stable' } }
  )

  const wasm = join(WASM_TARGET_DIR, `${stem}.wasm`)
  const pkg = join(GLUE_OUT, stem)
  rmSync(pkg, { recursive: true, force: true })
  mkdirSync(pkg, { recursive: true })
  run(wasmBindgen, ['--target', 'web', '--out-dir', pkg, wasm])

  const bg = join(pkg, `${stem}_bg.wasm`)
  const before = statSync(bg).size
  // -O3 (speed), NOT -Oz (size): match the native opt-3 profile so wasm-opt's
  // pass reinforces the cargo speed build instead of trading it back for bytes.
  run('wasm-opt', ['-O3', ...WASM_OPT_FEATURES, '-o', bg, bg])
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

function writeArtifactPin() {
  const sourceCommit = execFileSync('git', ['-C', join(ROOT, 'rust/aterm'), 'rev-parse', 'HEAD'], {
    encoding: 'utf8'
  }).trim()
  const artifacts = {}
  for (const file of ARTIFACTS) {
    const bytes = readFileSync(join(DEST, file))
    artifacts[file] = {
      bytes: bytes.byteLength,
      sha256: createHash('sha256').update(bytes).digest('hex')
    }
  }
  writeFileSync(
    join(DEST, ARTIFACT_PIN),
    `${JSON.stringify({ schema: 1, sourceCommit, artifacts }, null, 2)}\n`
  )
  console.log(`[aterm-wasm] pinned ${ARTIFACTS.length} artifacts to ${sourceCommit}`)
}

function atermSourceIsClean() {
  return (
    execFileSync('git', ['-C', join(ROOT, 'rust/aterm'), 'status', '--porcelain'], {
      encoding: 'utf8'
    }).trim().length === 0
  )
}

if (!which('wasm-opt')) {
  const install =
    process.platform === 'darwin'
      ? '`brew install binaryen`'
      : process.platform === 'win32'
        ? 'a binaryen release from https://github.com/WebAssembly/binaryen/releases (add its bin/ to PATH)'
        : 'the binaryen package (e.g. `apt install binaryen`) or a release from https://github.com/WebAssembly/binaryen/releases'
  console.error(`[aterm-wasm] wasm-opt not found — install ${install}`)
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
if (Object.keys(CRATES).every((key) => keys.includes(key)) && atermSourceIsClean()) {
  writeArtifactPin()
} else if (Object.keys(CRATES).every((key) => keys.includes(key))) {
  console.log(`[aterm-wasm] dirty aterm source: ${ARTIFACT_PIN} intentionally left unchanged`)
} else {
  console.log(`[aterm-wasm] partial build: ${ARTIFACT_PIN} intentionally left unchanged`)
}
console.log('\n[aterm-wasm] done.')
