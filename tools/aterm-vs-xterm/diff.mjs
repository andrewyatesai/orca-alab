import { readFileSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
const HERE = '/Users/ayates/orc/tools/aterm-vs-xterm'
const ATERM = '/Users/ayates/orc/rust/aterm/target/release/examples/snapshot'
const corpus = JSON.parse(readFileSync(`${HERE}/corpus.json`))
const run = (cmd, args, buf) =>
  execFileSync(cmd, args, { input: buf, maxBuffer: 1 << 20, timeout: 5000 }).toString()
let match = 0
const diverge = []
for (const { name, bytes } of corpus) {
  const buf = Buffer.from(bytes, 'latin1')
  let a, x
  try {
    a = run(ATERM, [], buf)
  } catch (e) {
    a = `ATERM-ERR:${e.killed ? 'TIMEOUT/KILL' : e.message.split('\n')[0]}`
  }
  try {
    x = run('node', [`${HERE}/snapshot.mjs`], buf)
  } catch (e) {
    x = `XTERM-ERR:${e.killed ? 'TIMEOUT/KILL' : e.message.split('\n')[0]}`
  }
  const ok = a === x
  if (ok) {
    match++
  } else {
    diverge.push({ name, a: a.split('\n'), x: x.split('\n') })
  }
  process.stderr.write(`[${ok ? 'OK  ' : 'DIFF'}] ${name}\n`)
}
console.log(`\nCONFORMANCE: ${match}/${corpus.length} byte-identical`)
for (const d of diverge) {
  console.log(`\n=== DIVERGE: ${d.name} ===`)
  for (let i = 0; i < 24; i++) {
    if (d.a[i] !== d.x[i]) {
      console.log(
        `  row ${i}:  aterm=[${(d.a[i] ?? '').slice(0, 60)}]  xterm=[${(d.x[i] ?? '').slice(0, 60)}]`
      )
    }
  }
}
