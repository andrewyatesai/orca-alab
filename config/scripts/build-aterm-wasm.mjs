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
import {
  assertAtermWasmSourcePatchApplies,
  expectedAtermWasmSourcePatch,
  withPatchedAtermWorktree
} from './aterm-wasm-source-patch.mjs'
import { CargoCommandFailure, runStreamedCargoCommand } from './stream-cargo-command.mjs'
import { assertNoEmbeddedLocalBuildPaths, wasmPathRemapRustflags } from './wasm-build-paths.mjs'

const ROOT = join(import.meta.dirname, '..', '..')
const DEST = join(ROOT, 'src/renderer/src/lib/pane-manager/aterm')
const ATERM_SOURCE = join(ROOT, 'rust/aterm')
// Cargo output remains shared with the normal submodule build even though the
// patched source is compiled from a detached temporary worktree.
const CARGO_TARGET_DIR = join(ATERM_SOURCE, 'target')
const WASM_TARGET_DIR = join(CARGO_TARGET_DIR, 'wasm32-unknown-unknown/release')
const GLUE_OUT = join(CARGO_TARGET_DIR, 'aterm-web-glue')
const WB_VERSION = '0.2.108'
const WB_DIR = join(ROOT, 'config/.tooling', `wasm-bindgen-${WB_VERSION}`)
const ARTIFACT_PIN = 'aterm_wasm_artifact_pin.json'
// wasm-opt rejects the module unless the features it uses are enabled explicitly.
const WASM_OPT_FEATURES = [
  '--enable-bulk-memory',
  '--enable-nontrapping-float-to-int',
  '--enable-sign-ext',
  '--enable-mutable-globals',
  '--enable-reference-types',
  '--enable-simd'
]
// simd128 is the prerequisite for aterm's v128 scanners (upstream work) and already
// activates memchr's wasm-simd paths + LLVM autovectorization; scalar behavior unchanged.
const WASM_SIMD_RUSTFLAG = '-C target-feature=+simd128'

const CRATES = {
  cpu: { dir: 'crates/aterm-wasm', stem: 'aterm_wasm' },
  gpu: { dir: 'crates/aterm-gpu-web', stem: 'aterm_gpu_web' }
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
async function runWasmCargo(args, opts = {}) {
  const baseEnv = opts.env ?? process.env
  if (which('rustup')) {
    const cargo = rustupStableBin('cargo')
    const rustc = rustupStableBin('rustc')
    await runStreamedCargoCommand({
      command: cargo,
      args,
      cwd: opts.cwd ?? ROOT,
      env: { ...baseEnv, RUSTC: rustc },
      label: 'aterm-wasm'
    })
  } else {
    await runStreamedCargoCommand({
      command: 'cargo',
      args,
      cwd: opts.cwd ?? ROOT,
      env: baseEnv,
      label: 'aterm-wasm',
      shell: process.platform === 'win32'
    })
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

async function buildCrate(key, wasmBindgen, atermSource) {
  const { dir, stem } = CRATES[key]
  console.log(`\n[aterm-wasm] building ${key} (${dir}) …`)
  // Build from ROOT (online ancestry) via --manifest-path so the web deps
  // resolve from crates.io, not the offline rust/vendor. Inherit aterm's native
  // [profile.release] as-is: opt-level=3 + fat LTO. The per-frame render, glyph
  // layout, and ALL effects run in this wasm every frame (even on the WebGL2 GPU
  // path), so hot-loop speed drives animation smoothness — a size-opt override
  // (opt-level="z") made the wasm visibly chunkier than the native opt-3 build.
  // A few MB more download is a non-issue; smoothness is the product.
  await runWasmCargo(
    [
      'build',
      '--release',
      '--target',
      'wasm32-unknown-unknown',
      '--manifest-path',
      join(atermSource, dir, 'Cargo.toml')
    ],
    // Pin to stable (aterm's rust-toolchain.toml channel): the machine's global rustup
    // default may be an older nightly that lacks the wasm32-unknown-unknown target (or
    // violates aterm's rust-version). RUSTUP_TOOLCHAIN is a belt-and-suspenders for the
    // no-rustup fallback path. The simd flag is target-scoped so host proc-macro builds
    // stay untouched (plain RUSTFLAGS would leak into them).
    {
      env: {
        ...process.env,
        CARGO_TARGET_DIR,
        CARGO_NET_OFFLINE: 'false',
        RUSTUP_TOOLCHAIN: 'stable',
        CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS: [
          process.env.CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS,
          WASM_SIMD_RUSTFLAG,
          ...wasmPathRemapRustflags({ root: ROOT, atermSource })
        ]
          .filter(Boolean)
          .join(' ')
      }
    }
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
  assertNoEmbeddedLocalBuildPaths(readFileSync(bg), {
    root: ROOT,
    atermSource,
    label: `${stem}_bg.wasm`
  })
  const after = statSync(bg).size
  console.log(
    `[aterm-wasm] ${stem}_bg.wasm ${before} -> ${after} bytes ` +
      `(-${(((before - after) * 100) / before).toFixed(1)}% via wasm-opt)`
  )

  // Glue-parity gate: identical Rust shim bodies get folded by LLVM
  // MergeFunctions, after which wasm-bindgen binds two JS methods to ONE
  // surviving export (observed: predict_reset silently calling
  // predict_line_submit). The engine carries black_box ICF barriers, but a
  // regression here misroutes calls with zero build error — so assert every
  // simple `name() { wasm.<...>_name(...) }` method calls its OWN export.
  const glue = readFileSync(join(pkg, `${stem}.js`), 'utf8')
  const misbound = []
  for (const m of glue.matchAll(
    /^\s{4}(\w+)\(\) \{\n\s*wasm\.\w*?terminal_(\w+)\(this\.__wbg_ptr\);/gm
  )) {
    if (m[1] !== m[2]) {
      misbound.push(`${m[1]}() -> ${m[2]}`)
    }
  }
  if (misbound.length > 0) {
    console.error(
      `[aterm-wasm] FATAL: ${stem} glue cross-binding (merged exports?): ${misbound.join(', ')}`
    )
    process.exit(1)
  }

  for (const ext of ['.js', '.d.ts', '_bg.wasm', '_bg.wasm.d.ts']) {
    copyFileSync(join(pkg, `${stem}${ext}`), join(DEST, `${stem}${ext}`))
  }
  console.log(`[aterm-wasm] copied ${stem} glue → src/renderer/.../aterm/`)
}

function writeArtifactPin(sourceCommit, sourcePatch) {
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
    `${JSON.stringify({ schema: 2, sourceCommit, sourcePatch, artifacts }, null, 2)}\n`
  )
  console.log(
    `[aterm-wasm] pinned ${ARTIFACTS.length} artifacts to ${sourceCommit} + ` +
      `${sourcePatch.path}@${sourcePatch.sha256.slice(0, 12)}`
  )
}

function atermSourceIsClean() {
  return (
    execFileSync('git', ['-C', ATERM_SOURCE, 'status', '--porcelain'], {
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
try {
  for (const k of keys) {
    if (!CRATES[k]) {
      console.error(`[aterm-wasm] unknown target "${k}" (use --cpu and/or --gpu)`)
      process.exit(1)
    }
  }
  if (!atermSourceIsClean()) {
    throw new Error(
      'rust/aterm has uncommitted source changes; refusing to build artifacts with ambiguous provenance'
    )
  }
  const sourceCommit = execFileSync('git', ['-C', ATERM_SOURCE, 'rev-parse', 'HEAD'], {
    encoding: 'utf8'
  }).trim()
  const sourcePatch = expectedAtermWasmSourcePatch(ROOT)
  assertAtermWasmSourcePatchApplies(ROOT, ATERM_SOURCE)

  await withPatchedAtermWorktree(
    { root: ROOT, atermSource: ATERM_SOURCE, sourceCommit },
    async (patchedAtermSource) => {
      for (const k of keys) {
        await buildCrate(k, wasmBindgen, patchedAtermSource)
      }
    }
  )

  if (Object.keys(CRATES).every((key) => keys.includes(key))) {
    writeArtifactPin(sourceCommit, sourcePatch)
  } else {
    console.log(`[aterm-wasm] partial build: ${ARTIFACT_PIN} intentionally left unchanged`)
  }
  console.log('\n[aterm-wasm] done.')
} catch (error) {
  if (!(error instanceof CargoCommandFailure)) {
    throw error
  }
  console.error(`[aterm-wasm] ${error.message}`)
  process.exitCode = error.exitCode
}
