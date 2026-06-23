// Build + size-optimize the aterm renderer wasm glue (CPU `aterm-wasm` and GPU
// `aterm-gpu-web`) and copy it into the renderer. Each crate is built
// opt-level="z"+lto+strip (see its Cargo.toml), bindgen'd with the matching
// wasm-bindgen, then run through `wasm-opt -Oz`. The committed glue is the
// optimized output, so this script makes that reproducible instead of manual.
//
// Usage: node config/scripts/build-aterm-wasm.mjs [--cpu] [--gpu]  (default: both)
import { execFileSync } from 'node:child_process'
import { existsSync, copyFileSync, statSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..')
const DEST = join(ROOT, 'src/renderer/src/lib/pane-manager/aterm')
// wasm-opt rejects the module unless the features it uses are enabled explicitly.
const WASM_OPT_FEATURES = [
  '--enable-bulk-memory',
  '--enable-nontrapping-float-to-int',
  '--enable-sign-ext',
  '--enable-mutable-globals',
  '--enable-reference-types'
]

const CRATES = {
  cpu: { dir: 'native/aterm-wasm', stem: 'aterm_wasm' },
  gpu: { dir: 'native/aterm-gpu-web', stem: 'aterm_gpu_web' }
}

function run(cmd, args, cwd) {
  execFileSync(cmd, args, { cwd, stdio: 'inherit' })
}

function resolveWasmBindgen(crateDir) {
  // The GPU crate pins a newer wasm-bindgen in a local .wbtool/ (it must match
  // the crate's pinned dep); fall back to the one on PATH for the CPU crate.
  const pinned = join(crateDir, '.wbtool/bin/wasm-bindgen')
  return existsSync(pinned) ? pinned : 'wasm-bindgen'
}

function which(bin) {
  try {
    execFileSync('sh', ['-c', `command -v ${bin}`], { stdio: 'ignore' })
    return true
  } catch {
    return false
  }
}

function buildCrate(key) {
  const { dir, stem } = CRATES[key]
  const crateDir = join(ROOT, dir)
  const pkg = join(crateDir, 'pkg-web')
  const wasm = join(crateDir, `target/wasm32-unknown-unknown/release/${stem}.wasm`)
  console.log(`\n[aterm-wasm] building ${key} (${dir}) …`)
  run('cargo', ['build', '--release', '--target', 'wasm32-unknown-unknown'], crateDir)
  run(resolveWasmBindgen(crateDir), ['--target', 'web', '--out-dir', pkg, wasm], crateDir)

  const bg = join(pkg, `${stem}_bg.wasm`)
  const before = statSync(bg).size
  run('wasm-opt', ['-Oz', ...WASM_OPT_FEATURES, '-o', bg, bg], crateDir)
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
const flags = process.argv.slice(2)
const keys = flags.length ? flags.map((f) => f.replace(/^--/, '')) : ['cpu', 'gpu']
for (const k of keys) {
  if (!CRATES[k]) {
    console.error(`[aterm-wasm] unknown target "${k}" (use --cpu and/or --gpu)`)
    process.exit(1)
  }
  buildCrate(k)
}
console.log('\n[aterm-wasm] done.')
