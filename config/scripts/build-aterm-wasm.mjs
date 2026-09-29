// Build + speed-optimize the aterm renderer wasm glue (CPU `aterm-wasm` and GPU
// `aterm-gpu-web`) and copy it into the renderer. Speed over size: the hot render
// loop runs in this wasm every frame, so it inherits aterm's native opt-3 profile.
//
// The crates LIVE in aterm (vendored at rust/aterm/crates/*), so this builds
// them from there. Three wrinkles handled here:
//  1. Offline vendor: rust/.cargo/config.toml replaces crates-io with the
//     offline rust/vendor (which intentionally lacks the web deps wgpu-webgl/
//     wasm-bindgen/web-sys). cargo reads config from the INVOCATION CWD, not the
//     manifest — so we invoke from the repo ROOT (which has no vendoring config)
//     with CARGO_NET_OFFLINE=false to resolve the web deps online.
//  2. wasm-bindgen pin: both crates use =0.2.108; we use a cached CLI under
//     config/.tooling (bootstrapped via `targo --unverified install` if missing).
//  3. STOCK EXCEPTION (Trust lacks a wasm32-unknown-unknown std; measured
//     E0463 "can't find crate for `core`", and -Zbuild-std fails because the
//     seal ships no rust-src): the wasm32 build uses an INSTALLED stock rustup
//     toolchain (ORCA_STOCK_RUST_TOOLCHAIN, default `stable`), never installing
//     one — see rust-toolchain-lane.mjs.
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
import { cachedWasmBindgenExecutablePath } from './rust-host-executable-paths.mjs'
import {
  RustToolchainError,
  cargoInvocation,
  stockExceptionToolchain
} from './rust-toolchain-lane.mjs'
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

// STOCK EXCEPTION (Trust lacks a wasm32-unknown-unknown std; measured E0463):
// the stock cargo and rustc are pinned by absolute path (a Homebrew cargo on PATH
// ignores RUSTUP_TOOLCHAIN, and cargo spawns a BARE `rustc` unless RUSTC is set).
// No rustup, no toolchain, or no wasm32 target is an error naming what to
// install — never an install, and never a bare `cargo` fallback.
async function runWasmCargo(args, opts = {}) {
  const stock = stockExceptionToolchain({
    targets: ['wasm32-unknown-unknown'],
    env: opts.env ?? process.env
  })
  await runStreamedCargoCommand({
    command: stock.cargo,
    args,
    cwd: opts.cwd ?? ROOT,
    env: stock.env,
    label: 'aterm-wasm'
  })
}

function resolveWasmBindgen() {
  const cached = cachedWasmBindgenExecutablePath(WB_DIR)
  if (existsSync(cached)) {
    return cached
  }
  // Bootstrap the exact pinned CLI once (cached, gitignored) so the build is
  // reproducible regardless of the system wasm-bindgen version. It is a HOST tool,
  // so it builds with targo (`targo --unverified install`) where Trust serves the host.
  console.log(`[aterm-wasm] bootstrapping wasm-bindgen-cli ${WB_VERSION} → ${WB_DIR}`)
  const install = cargoInvocation({
    verb: 'install',
    args: ['wasm-bindgen-cli', '--version', WB_VERSION, '--root', WB_DIR, '--locked'],
    label: 'aterm-wasm'
  })
  run(install.command, install.args, { env: install.env })
  return cached
}

async function buildCrate(key, wasmBindgen, atermSource) {
  const { dir, stem } = CRATES[key]
  console.log(`\n[aterm-wasm] building ${key} (${dir}) …`)
  // Build from ROOT (online ancestry) via --manifest-path so the web deps
  // resolve from crates.io, not the offline rust/vendor. Inherit aterm's
  // [profile.release] as-is: fat LTO + `strip = true` (which is why the shipped
  // blobs carry no name section), with the two web crates ALREADY overridden to
  // opt-level="z" by [profile.release.package.*] — only their dependency closure
  // (aterm-core/-render/-effects, wgpu) stays at opt-level 3. That split is
  // deliberate: the per-frame render, glyph layout, and ALL effects run in this
  // wasm every frame (even on the WebGL2 GPU path), so hot-loop speed drives
  // animation smoothness, and a whole-closure size override was measured to make
  // the wasm visibly chunkier. A few MB more download is a non-issue.
  await runWasmCargo(
    [
      'build',
      '--release',
      '--target',
      'wasm32-unknown-unknown',
      '--manifest-path',
      join(atermSource, dir, 'Cargo.toml')
    ],
    // runWasmCargo pins the STOCK EXCEPTION toolchain (the machine's global
    // default may lack wasm32-unknown-unknown or violate aterm's rust-version).
    // The simd flag is target-scoped so host proc-macro builds stay untouched.
    {
      env: {
        ...process.env,
        CARGO_TARGET_DIR,
        CARGO_NET_OFFLINE: 'false',
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
  // -O3 (speed), NOT -Oz (size): match the dependency closure's opt-3 profile so
  // wasm-opt's pass reinforces the cargo speed build instead of trading it back.
  //
  // Post-pass size knobs are EXHAUSTED — measured 2026-07-31 on the shipped blobs
  // (reproduce with tools/wasm-blob-composition.mjs + wasm-opt against a copy):
  // a second -O3 recovers 0.06%; -Oz on top only 0.9% (CPU) / 1.6% (GPU) and costs
  // speed; --strip-debug/--strip-dwarf find nothing (release build); the `name`
  // section is already gone via profile `strip = true`; and --strip-producers
  // /--strip-target-features frees just 274 B while binaryen's re-encode adds
  // ~4.4 KB, so stripping them is a net LOSS. Nor is panic="abort" a lever:
  // wasm32-unknown-unknown already defaults to it. The remaining bytes are
  // structural (code 79-81%, data 18-21%) and only move engine-side.
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
let wasmBindgen
try {
  // Fail fast, before a multi-minute CLI bootstrap, when the STOCK EXCEPTION
  // toolchain for wasm32 is not installed.
  stockExceptionToolchain({ targets: ['wasm32-unknown-unknown'] })
  wasmBindgen = resolveWasmBindgen()
} catch (error) {
  if (!(error instanceof RustToolchainError)) {
    throw error
  }
  console.error(`[aterm-wasm] ${error.message}`)
  process.exit(1)
}
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
  if (error instanceof RustToolchainError) {
    console.error(`[aterm-wasm] ${error.message}`)
    process.exitCode = 1
  } else if (error instanceof CargoCommandFailure) {
    console.error(`[aterm-wasm] ${error.message}`)
    process.exitCode = error.exitCode
  } else {
    throw error
  }
}
