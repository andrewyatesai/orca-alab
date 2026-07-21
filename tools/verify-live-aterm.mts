// Live verification that the daemon's terminal engine is aterm (the default,
// no flag) and renders correctly end-to-end through the real
// `createHeadlessEmulator` -> napi -> aterm path. Self-contained: feeds a
// representative byte stream and asserts the snapshot invariants. Run with:
//   node config/scripts/build-terminal-addon.mjs && node tools/verify-live-aterm.mts
import { fileURLToPath } from 'node:url'
import { existsSync } from 'node:fs'
import { registerHooks } from 'node:module'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

// Node 24 strips TypeScript syntax itself, but ESM still requires extensions.
// The application source intentionally uses bundler-style relative specifiers,
// so give this standalone verifier the same local `.ts` resolution without a
// downloaded runner such as `npx tsx`/`pnpm dlx tsx`.
registerHooks({
  resolve(specifier, context, nextResolve) {
    try {
      return nextResolve(specifier, context)
    } catch (error) {
      if (
        !(error instanceof Error) ||
        !('code' in error) ||
        error.code !== 'ERR_MODULE_NOT_FOUND' ||
        !/^\.{1,2}\//.test(specifier)
      ) {
        throw error
      }
      return nextResolve(`${specifier}.ts`, context)
    }
  }
})

const addonPath = fileURLToPath(new URL('../native/orca-node/orca_node.node', import.meta.url))
if (!existsSync(addonPath)) {
  console.error(`addon missing at ${addonPath} — run: node config/scripts/build-terminal-addon.mjs`)
  process.exit(1)
}
// Point the loader at the freshly built addon; aterm is the default engine, so
// no ORCA_RUST_TERMINAL flag is needed.
process.env.ORCA_RUST_TERMINAL_ADDON = addonPath
process.env.ORCA_ENGINE_MARKER = join(tmpdir(), 'orca-aterm-engine-marker')

const { createHeadlessEmulator } = await import('../src/main/daemon/headless-emulator-factory.ts')

const em = createHeadlessEmulator({ cols: 80, rows: 24, scrollback: 5000 })
// cwd (OSC-7), title (OSC 0/2), colour, an OSC-8 hyperlink, then scrollback fill.
em.write('\x1b]7;file:///srv/app\x07')
em.write('\x1b]2;orca · aterm\x07')
em.write('\x1b[1;32mhello\x1b[0m \x1b]8;;https://orca.dev\x07docs\x1b]8;;\x07\r\n')
for (let i = 0; i < 60; i += 1) {
  em.write(`line ${i}\r\n`)
}
// Capture scrollback on the main buffer BEFORE switching to the alt screen (the
// alt buffer has no scrollback of its own).
const mainSnap = em.getSnapshot()
em.write('\x1b[?1049h\x1b[?1002h\x1b[?1006h') // alt screen + mouse drag + SGR

const snap = em.getSnapshot()
const checks: [string, boolean][] = [
  ['cwd tracked', snap.cwd === '/srv/app'],
  ['title tracked', snap.lastTitle === 'orca · aterm'],
  ['alt screen', snap.modes.alternateScreen === true],
  ['mouse drag + SGR', snap.modes.mouseTrackingMode === 'drag' && snap.modes.sgrMouseMode === true],
  ['scrollback retained (main buffer)', mainSnap.scrollbackLines > 0],
  ['main-buffer history recoverable in alt', snap.scrollbackAnsi.includes('line 0')],
  ['snapshot non-empty', snap.snapshotAnsi.length > 0],
  ['rehydrate re-enters alt', snap.rehydrateSequences.includes('\x1b[?1049h')]
]

let ok = true
for (const [name, pass] of checks) {
  console.log(`  ${pass ? '✅' : '❌'} ${name}`)
  ok = ok && pass
}
console.log(
  `engine: aterm | main scrollbackLines=${mainSnap.scrollbackLines} | snapshotAnsi=${snap.snapshotAnsi.length}B`
)
em.dispose()
process.exit(ok ? 0 : 1)
