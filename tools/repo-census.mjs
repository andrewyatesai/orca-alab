#!/usr/bin/env node
// Generated repo census: the single source of truth for inventory numbers cited in
// docs/rust-migration/{extreme-performance-moonshot,orca-from-scratch-blueprint}.md.
// Hand-counted inventory goes stale (a 2026-07-15 external review caught drifted LOC/IPC
// counts) — this script regenerates every number reproducibly. Candidate gauntlet axis.
//
// Usage: node tools/repo-census.mjs [--json <outfile>]

import { execFileSync } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'

const repo = path.resolve(import.meta.dirname, '..')

const tracked = execFileSync('git', ['ls-files'], { cwd: repo, maxBuffer: 64 * 1024 * 1024 })
  .toString()
  .split('\n')
  .filter(Boolean)

const isTs = (f) => /\.(ts|tsx|mts|cts)$/.test(f)
const isTest = (f) =>
  /\.(test|spec)\.(ts|tsx|mts|cts)$/.test(f) || /(^|\/)(tests?|__tests__)\//.test(f)

const lineCache = new Map()
function lines(file) {
  if (!lineCache.has(file)) {
    try {
      const buf = readFileSync(path.join(repo, file))
      let n = 0
      for (let i = 0; i < buf.length; i++) {
        if (buf.at(i) === 10) {
          n++
        }
      }
      // count a trailing unterminated line
      if (buf.length > 0 && buf.at(-1) !== 10) {
        n++
      }
      lineCache.set(file, n)
    } catch {
      lineCache.set(file, 0)
    }
  }
  return lineCache.get(file)
}

function locArea(prefix) {
  let src = 0,
    srcFiles = 0,
    test = 0,
    testFiles = 0
  for (const f of tracked) {
    if (!f.startsWith(prefix) || !isTs(f)) {
      continue
    }
    if (isTest(f)) {
      test += lines(f)
      testFiles++
    } else {
      src += lines(f)
      srcFiles++
    }
  }
  return { srcLoc: src, srcFiles, testLoc: test, testFiles }
}

function grepChannels(files, regex) {
  const set = new Set()
  let calls = 0
  for (const f of files) {
    const text = readFileSync(path.join(repo, f), 'utf8')
    for (const m of text.matchAll(regex)) {
      calls++
      if (m[1]) {
        set.add(m[1])
      }
    }
  }
  return { calls, distinct: set.size }
}

const mainTs = tracked.filter((f) => f.startsWith('src/main/') && isTs(f) && !isTest(f))
const preloadRendererTs = tracked.filter(
  (f) => (f.startsWith('src/preload/') || f.startsWith('src/renderer/')) && isTs(f) && !isTest(f)
)

const areas = {
  'src/renderer': locArea('src/renderer/'),
  'src/main': locArea('src/main/'),
  'src/shared': locArea('src/shared/'),
  'src/preload': locArea('src/preload/'),
  mobile: locArea('mobile/')
}

const ipc = {
  ipcMainRegistrations: grepChannels(mainTs, /ipcMain\.(?:handle|on)\(\s*['"`]([^'"`\n]+)['"`]?/g),
  invokeChannels: grepChannels(
    preloadRendererTs,
    /ipcRenderer\.invoke\(\s*['"`]([^'"`\n]+)['"`]?/g
  ),
  rendererSendChannels: grepChannels(
    preloadRendererTs,
    /ipcRenderer\.send\(\s*['"`]([^'"`\n]+)['"`]?/g
  ),
  rendererOnChannels: grepChannels(
    preloadRendererTs,
    /ipcRenderer\.on\(\s*['"`]([^'"`\n]+)['"`]?/g
  ),
  webContentsSendChannels: grepChannels(mainTs, /\.send\(\s*['"`]([a-zA-Z][\w:.-]+)['"`]/g)
}

const mainCss = 'src/renderer/src/assets/main.css'
const cssText = readFileSync(path.join(repo, mainCss), 'utf8')
// unique names, not declaration lines — light/dark redefine the same token
const tokenNames = new Set([...cssText.matchAll(/^\s*(--[A-Za-z][\w-]*)\s*:/gm)].map((m) => m[1]))
const designTokens = tokenNames.size
const tokenDeclarations = [...cssText.matchAll(/^\s*--[A-Za-z][\w-]*\s*:/gm)].length

const uiPrimitives = tracked.filter(
  (f) => f.startsWith('src/renderer/src/components/ui/') && isTs(f) && !isTest(f)
).length

const rpcMethodFiles = tracked.filter(
  (f) => f.startsWith('src/main/runtime/rpc/methods/') && isTs(f) && !isTest(f)
)
const rpcDefineMethods = grepChannels(
  rpcMethodFiles,
  /defineMethod(?:<[^>]*>)?\(\s*['"`]?([\w.:-]+)?/g
)

const bigFiles = tracked
  .filter((f) => isTs(f) && (f.startsWith('src/') || f.startsWith('mobile/')))
  .map((f) => ({ file: f, lines: lines(f) }))
  .sort((a, b) => b.lines - a.lines)
  .slice(0, 12)

// The PTY delivery-reliability layer: files whose whole purpose is compensating for
// unguaranteed push delivery. A daemon-first subscribe/resume protocol deletes the class.
// In-file regions inside pty.ts are NOT counted here (hand-measured separately) — this
// manifest is the reproducible, whole-file portion of the deletion claim.
const shimBasenames = [
  'pty-hidden-delivery-gate.ts',
  'pty-producer-flow-control.ts',
  'daemon-stream-data-batcher.ts',
  'daemon-stream-keep-tail-drop.ts',
  'daemon-stream-backlog-probe.ts',
  'terminal-delivery-watchdog.ts',
  'pty-dispatcher.ts',
  'terminal-pty-ack-gate.ts',
  'pty-model-restore-channel.ts',
  'hidden-output-restore-scheduler.ts',
  'pty-delivery-interest.ts',
  'terminal-freeze-breadcrumbs.ts',
  'pty-pre-handler-buffer.ts',
  'pty-renderer-delivery-health.ts',
  'pty-delivery-diagnostics.ts',
  'replay-guard.ts',
  'terminal-write-pipeline-health.ts',
  'xterm-write-callback-guard.ts',
  'binary-frame.ts'
]
const shimManifest = shimBasenames.flatMap((base) =>
  tracked
    .filter((f) => f.endsWith(`/${base}`) && !isTest(f) && f.startsWith('src/'))
    .map((f) => ({ file: f, lines: lines(f) }))
)
const shimTotal = shimManifest.reduce((a, b) => a + b.lines, 0)

// Ratchet-watched files: the god object and the delivery-shim host — the rebuild
// thesis says these only shrink; the gauntlet census axis enforces the direction.
const watched = ['src/main/runtime/orca-runtime.ts', 'src/main/ipc/pty.ts']
const watchedFiles = Object.fromEntries(watched.map((f) => [f, lines(f)]))

const census = {
  generatedBy: 'tools/repo-census.mjs',
  gitHead: execFileSync('git', ['rev-parse', '--short', 'HEAD'], { cwd: repo }).toString().trim(),
  areas,
  ipc,
  watchedFiles,
  designSystem: {
    mainCssLines: lines(mainCss),
    designTokens,
    tokenDeclarations,
    uiPrimitiveFiles: uiPrimitives
  },
  mobileRpc: { methodFiles: rpcMethodFiles.length, defineMethodCalls: rpcDefineMethods.calls },
  bigFiles,
  deliveryReliabilityShim: { wholeFileTotalLoc: shimTotal, files: shimManifest }
}

const jsonIdx = process.argv.indexOf('--json')
if (jsonIdx !== -1 && process.argv[jsonIdx + 1]) {
  writeFileSync(process.argv[jsonIdx + 1], `${JSON.stringify(census, null, 2)}\n`)
}

const fmt = (n) => n.toLocaleString('en-US')
console.log(`repo census @ ${census.gitHead}`)
for (const [k, v] of Object.entries(areas)) {
  console.log(
    `  ${k}: ${fmt(v.srcLoc)} LOC / ${fmt(v.srcFiles)} files (+ ${fmt(v.testLoc)} test LOC / ${fmt(v.testFiles)} files)`
  )
}
console.log(
  `  ipcMain registrations: ${ipc.ipcMainRegistrations.calls} (${ipc.ipcMainRegistrations.distinct} distinct channels)`
)
console.log(
  `  ipcRenderer.invoke: ${ipc.invokeChannels.calls} calls / ${ipc.invokeChannels.distinct} distinct; .send ${ipc.rendererSendChannels.distinct} distinct; .on ${ipc.rendererOnChannels.distinct} distinct`
)
console.log(`  main->renderer send channels: ${ipc.webContentsSendChannels.distinct} distinct`)
console.log(
  `  design tokens: ${designTokens} unique (${tokenDeclarations} declarations, main.css ${fmt(lines(mainCss))} lines); ui primitives: ${uiPrimitives}`
)
console.log(
  `  mobile rpc: ${rpcMethodFiles.length} method files, ${rpcDefineMethods.calls} defineMethod calls`
)
console.log(
  `  reliability shim (whole files): ${fmt(shimTotal)} LOC across ${shimManifest.length} files`
)
console.log('  largest files:')
for (const f of bigFiles) {
  console.log(`    ${fmt(f.lines).padStart(7)}  ${f.file}`)
}
