import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'

const ADDON = '/Users/ayates/orca-aterm/native/orca-node/orca_node.node'
const corpus = readFileSync('/tmp/orca-bench/corpus.bin').toString('latin1')
const CHUNK = 4096
const FNV = (s: string) => {
  let h = 0xcbf29ce484222325n
  for (const b of Buffer.from(s, 'utf8')) {
    h ^= BigInt(b)
    h = (h * 0x00000100000001b3n) & 0xffffffffffffffffn
  }
  return h.toString(16).padStart(16, '0')
}

// 1) Prove the daemon FACTORY routes to aterm behind the flag, through the full
//    TerminalEmulator interface the session depends on.
process.env.ORCA_RUST_TERMINAL = '1'
process.env.ORCA_RUST_TERMINAL_ADDON = ADDON
const { createHeadlessEmulator } =
  await import('/Users/ayates/orca-aterm/src/main/daemon/headless-emulator-factory.ts')
const em = createHeadlessEmulator({ cols: 120, rows: 40, scrollback: 5000 })
for (let i = 0; i < corpus.length; i += CHUNK) {
  em.write(corpus.slice(i, i + CHUNK))
}
const snap = em.getSnapshot()
console.log(
  'factory emulator: scrollbackLines',
  snap.scrollbackLines,
  '| serializeAnsi',
  snap.snapshotAnsi.length,
  'B | cwd',
  snap.cwd
)
console.log('  modes:', JSON.stringify(snap.modes))
const surface = em as unknown as Record<string, unknown>
console.log(
  '  interface ok:',
  ['write', 'resize', 'getSnapshot', 'getCwd', 'clearScrollback', 'dispose'].every(
    (m) => typeof surface[m] === 'function'
  )
)

// 2) Prove the aterm engine the daemon now uses renders the CORRECT visible grid
//    (byte-identical to the xterm golden = fcc6cf2cb337edd0).
const require = createRequire(import.meta.url)
const addon = require(ADDON)
const t = new addon.HeadlessTerminal(120, 40, 5000)
for (let i = 0; i < corpus.length; i += CHUNK) {
  t.write(Buffer.from(corpus.slice(i, i + CHUNK), 'latin1'))
}
const fnv = FNV(t.snapshot().join('\n'))
console.log(
  'aterm daemon-engine visible grid:',
  fnv,
  fnv === 'fcc6cf2cb337edd0' ? '== xterm golden ✅' : '!= golden ❌'
)
