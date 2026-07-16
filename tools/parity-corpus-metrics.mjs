#!/usr/bin/env node
// Parity-corpus metrics — the F2 "publish Cedar-style corpus metrics" half of the
// autoformalization factory (docs/rust-migration/extreme-performance-moonshot.md).
//
// Turns the scattered behavioral parity corpora into ONE measured, reproducible
// headline: how many machine-checked (input -> expected-output) cases guard the
// TS<->Rust equivalence, across how many modules, all re-run every gauntlet pass.
// (Cedar publishes exactly this number as a reliability claim; the moonshot's
// "regression-gated behavioral parity corpora" phrase is only honest with a count.)
//
// Discovery is mechanical and covers BOTH corpus families — no hand-maintained list,
// so a new corpus is counted the moment it lands (the ad-hoc globs it replaces
// silently missed the plain `parity-corpus.txt` E1 names):
//   • dispatch-parity vectors — tools/parity/vectors/<module>.json, each a
//     {module, source, rustCrate, cases:[...]} table run by BOTH the TS reference
//     leg and the Rust vector module (orca-dispatch / orca-parity registries).
//   • E1 shared corpora — rust/crates/<crate>/*parity-corpus.txt, a text oracle read
//     line-for-line by BOTH the Rust core (matches_shared_parity_corpus) and its TS
//     twin's vitest parity test.
//
// Usage:
//   node tools/parity-corpus-metrics.mjs            human summary
//   node tools/parity-corpus-metrics.mjs --json     machine-readable metrics
//   node tools/parity-corpus-metrics.mjs --json out.json   also write the JSON

import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs'
import { join, relative, resolve } from 'node:path'

const repo = resolve(import.meta.dirname, '..')

// A corpus line that carries a case: not blank, not a `#` comment.
const isCaseLine = (line) => {
  const t = line.trim()
  return t !== '' && !t.startsWith('#')
}

function dispatchParityVectors() {
  const dir = join(repo, 'tools', 'parity', 'vectors')
  const modules = []
  if (!existsSync(dir)) {
    return modules
  }
  for (const file of readdirSync(dir)
    .filter((f) => f.endsWith('.json'))
    .sort()) {
    let cases = 0
    let source = null
    let rustCrate = null
    try {
      const v = JSON.parse(readFileSync(join(dir, file), 'utf8'))
      // Canonical shape is {module, source, rustCrate, cases:[...]}; tolerate a bare
      // array or a {vectors:[...]} variant so a format tweak doesn't zero the count.
      const arr = Array.isArray(v) ? v : (v.cases ?? v.vectors ?? [])
      cases = Array.isArray(arr) ? arr.length : 0
      source = v.source ?? null
      rustCrate = v.rustCrate ?? null
    } catch {
      cases = 0 // malformed JSON contributes 0 cases but is still reported as a module
    }
    modules.push({
      module: file.slice(0, -5),
      file: `tools/parity/vectors/${file}`,
      cases,
      source,
      rustCrate
    })
  }
  return modules
}

function e1SharedCorpora() {
  const cratesDir = join(repo, 'rust', 'crates')
  const corpora = []
  if (!existsSync(cratesDir)) {
    return corpora
  }
  for (const crate of readdirSync(cratesDir).sort()) {
    const crateDir = join(cratesDir, crate)
    if (!statSync(crateDir).isDirectory()) {
      continue
    }
    for (const f of readdirSync(crateDir)
      .filter((n) => n.endsWith('parity-corpus.txt'))
      .sort()) {
      const cases = readFileSync(join(crateDir, f), 'utf8').split('\n').filter(isCaseLine).length
      corpora.push({ crate, file: relative(repo, join(crateDir, f)), cases })
    }
  }
  return corpora
}

function collectMetrics() {
  const vectors = dispatchParityVectors()
  const e1 = e1SharedCorpora()
  const sum = (rows) => rows.reduce((n, r) => n + r.cases, 0)
  const dispatchCases = sum(vectors)
  const e1Cases = sum(e1)
  const e1Crates = new Set(e1.map((c) => c.crate)).size
  return {
    generatedBy: 'tools/parity-corpus-metrics.mjs',
    total: { modules: vectors.length + e1.length, cases: dispatchCases + e1Cases },
    dispatchParity: { modules: vectors.length, cases: dispatchCases },
    e1Shared: { corpora: e1.length, crates: e1Crates, cases: e1Cases },
    modules: { dispatchParity: vectors, e1Shared: e1 }
  }
}

function printSummary(m) {
  const { total, dispatchParity, e1Shared } = m
  console.log('Parity-corpus metrics — machine-checked TS<->Rust behavioral cases\n')
  console.log(
    `  TOTAL: ${total.cases} cases across ${total.modules} modules (re-run every gauntlet)`
  )
  console.log(
    `    • dispatch-parity vectors:  ${String(dispatchParity.cases).padStart(5)} cases / ${dispatchParity.modules} modules`
  )
  console.log(
    `    • E1 shared corpora:        ${String(e1Shared.cases).padStart(5)} cases / ${e1Shared.corpora} corpora (${e1Shared.crates} crates)`
  )
  const zero = m.modules.dispatchParity.filter((v) => v.cases === 0).map((v) => v.module)
  if (zero.length > 0) {
    console.log(`\n  note: ${zero.length} vector module(s) reported 0 cases: ${zero.join(', ')}`)
  }
}

function main() {
  const args = process.argv.slice(2)
  const m = collectMetrics()
  const outArg = args.find((a) => a.endsWith('.json') && a !== '--json')
  if (outArg) {
    writeFileSync(resolve(outArg), `${JSON.stringify(m, null, 2)}\n`)
  }
  if (args.includes('--json')) {
    console.log(JSON.stringify(m, null, 2))
  } else {
    printSummary(m)
  }
  return 0
}

process.exit(main())
