#!/usr/bin/env node
// Port-provenance manifest — pins every TS→Rust ported module to the sha256 of its
// TS source(s) and Rust twin(s), so upstream TS drift fails LOUDLY with a structured
// re-port task instead of being caught reactively by parity mid-merge.
//
// The mapping is derived mechanically (no hand-maintained table):
//   • parity registry — the match arms in rust/crates/{orca-dispatch,orca-parity}/
//     src/modules/mod.rs name every vector module; each module's TS sources come from
//     its tools/parity/dispatch/<module>.ts value-imports (type-only imports are
//     excluded — they cannot change the executed reference leg), and its Rust twins
//     from the dispatch adapter's `use orca_*::…` statements (root re-exports resolved
//     through the crate's lib.rs `pub use` map).
//   • ledger — docs/rust-migration/ported-modules.md table rows cover ported modules
//     with no parity adapter (IO tier); rows whose Rust twin is already parity-mapped
//     merge their extra TS sources into that entry.
// Anything not mechanically resolvable lands in the manifest's `unmapped` section
// honestly instead of being guessed. Files carrying merge-conflict markers are never
// hashed (the hash would pin junk); they are reported and the manifest regenerated
// once the merge resolves.
//
// Usage:
//   node tools/port-provenance.mjs --generate    derive + write the manifest
//   node tools/port-provenance.mjs [--json]      re-derive, diff against the committed
//                                                manifest; exit 0 clean / 2 on drift
// Env:
//   PROVENANCE_ROOT=<dir>  overlay root: any repo-relative path present under it is
//   read from there instead of the repo — drift-detection tests mutate a copied
//   fixture, never the tracked tree.

import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'

const repo = resolve(import.meta.dirname, '..')
const MANIFEST_REL = 'tools/terminal-bench/port-provenance.json'
const overlay = process.env.PROVENANCE_ROOT ? resolve(process.env.PROVENANCE_ROOT) : null

const readRel = (rel) => {
  if (overlay && existsSync(join(overlay, rel))) {
    return readFileSync(join(overlay, rel), 'utf8')
  }
  return existsSync(join(repo, rel)) ? readFileSync(join(repo, rel), 'utf8') : null
}
const existsRel = (rel) =>
  (overlay !== null && existsSync(join(overlay, rel))) || existsSync(join(repo, rel))
// Line endings are normalized so a CRLF checkout (Windows) hashes identically.
const sha256 = (text) =>
  createHash('sha256').update(text.replaceAll('\r\n', '\n').replaceAll('\r', '\n')).digest('hex')
const hasConflictMarkers = (text) => /^<{7} /mu.test(text)
const byPath = (a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0)

// --- parity registry: module list + per-module TS/Rust derivation -----------------

function registryArms(mod_rs_rel) {
  const src = readRel(mod_rs_rel)
  if (src === null) {
    throw new Error(`registry file missing: ${mod_rs_rel}`)
  }
  return [...src.matchAll(/"([\w-]+)" => Some\((\w+)::dispatch/gu)].map((m) => ({
    module: m[1],
    rustMod: m[2]
  }))
}

function parityTsSources(dispatchTsRel, unmapped, moduleId) {
  const src = readRel(dispatchTsRel)
  if (src === null) {
    unmapped.push({ id: moduleId, reason: `dispatch adapter missing: ${dispatchTsRel}` })
    return { files: [], oracle: false }
  }
  const files = new Set()
  let oracle = false
  for (const m of src.matchAll(/import\s+(type\s)?[^;]*?from\s+['"]([^'"]+)['"]/gu)) {
    const spec = m[2]
    if (m[1] !== undefined) {
      continue // type-only import: cannot change the executed TS reference leg
    }
    if (spec === './orca-git-wasm-oracle') {
      oracle = true // TS impl DELETED — the Rust port is the sole implementation
      continue
    }
    if (spec.startsWith('./') || !spec.startsWith('.')) {
      continue // harness-local module or bare package dependency, not a ported source
    }
    if (!spec.startsWith('../../../src/')) {
      unmapped.push({ id: moduleId, reason: `unresolvable dispatch import: ${spec}` })
      continue
    }
    const rel = spec.slice('../../../'.length)
    const hit = [`${rel}.ts`, `${rel}/index.ts`, rel].find((c) => existsRel(c))
    if (hit === undefined) {
      unmapped.push({ id: moduleId, reason: `dispatch import resolves to no file: ${spec}` })
    } else {
      files.add(hit)
    }
  }
  return { files: [...files], oracle }
}

const reexportCache = new Map()
function crateReexports(crateDir) {
  if (!reexportCache.has(crateDir)) {
    const map = new Map()
    const stars = []
    const lib = readRel(`${crateDir}/src/lib.rs`) ?? ''
    for (const m of lib.matchAll(/pub use\s+([a-z0-9_]+)::([^;]+);/gu)) {
      const rest = m[2].trim()
      const items = rest.startsWith('{') ? rest.slice(1, rest.lastIndexOf('}')).split(',') : [rest]
      for (const raw of items) {
        const item = raw.trim().replace(/\s+as\s+\w+$/u, '')
        if (item === '*') {
          stars.push(m[1])
        } else if (item !== '') {
          map.set(item, m[1])
        }
      }
    }
    reexportCache.set(crateDir, { map, stars })
  }
  return reexportCache.get(crateDir)
}

function rustModuleFile(crateDir, mod) {
  return [`${crateDir}/src/${mod}.rs`, `${crateDir}/src/${mod}/mod.rs`].find((c) => existsRel(c))
}

function parityRustTwins(dispatchRsRel, unmapped, moduleId) {
  const src = readRel(dispatchRsRel)
  if (src === null) {
    unmapped.push({ id: moduleId, reason: `dispatch adapter missing: ${dispatchRsRel}` })
    return []
  }
  const twins = new Set()
  // Some adapters delegate via inline fully-qualified paths with no `use` at all
  // (e.g. `orca_agents::tui_agent_startup_json::dispatch(...)`); scan those too,
  // over comment-stripped source so //! prose doesn't smuggle in mappings.
  const stripped = src.replaceAll(/\/\/[^\n]*/gu, '')
  for (const m of stripped.matchAll(/\b(orca_[a-z0-9_]+)::([a-z0-9_]+)/gu)) {
    const direct = rustModuleFile(`rust/crates/${m[1].replaceAll('_', '-')}`, m[2])
    if (direct !== undefined) {
      twins.add(direct)
    }
  }
  for (const m of src.matchAll(/use\s+(orca_[a-z0-9_]+)::([^;]+);/gu)) {
    const crateDir = `rust/crates/${m[1].replaceAll('_', '-')}`
    const rest = m[2].trim()
    // Heads: `mod::…` (first segment is a module) or `{item, …}` at crate root.
    const heads = rest.startsWith('{')
      ? rest
          .slice(1, rest.lastIndexOf('}'))
          .split(',')
          .map((s) => s.trim().split('::').at(0).trim())
          .filter((s) => s !== '')
      : [rest.split('::').at(0).trim()]
    for (const head of heads) {
      const name = head.replace(/\s+as\s+\w+$/u, '')
      const direct = rustModuleFile(crateDir, name)
      if (direct !== undefined) {
        twins.add(direct)
        continue
      }
      // Root re-export: resolve the defining module via the crate's lib.rs pub-use map.
      const { map, stars } = crateReexports(crateDir)
      const mod = map.get(name) ?? (stars.length === 1 ? stars.at(0) : undefined)
      const viaLib = mod === undefined ? undefined : rustModuleFile(crateDir, mod)
      if (viaLib !== undefined) {
        twins.add(viaLib)
        continue
      }
      // Single-file crate: the item is defined directly in the crate's lib.rs (no
      // submodule, no re-export). Pin lib.rs itself — the shape of the small E1
      // decision-core crates (orca-provider-backoff, orca-flow-control).
      const libRel = `${crateDir}/src/lib.rs`
      const libSrc = readRel(libRel)
      const definedInLib =
        libSrc !== null &&
        new RegExp(
          String.raw`pub (?:fn|const|static|struct|enum|trait|type)\s+${name}\b`,
          'u'
        ).test(libSrc)
      if (definedInLib) {
        twins.add(libRel)
      } else {
        unmapped.push({ id: moduleId, reason: `unresolved rust import: ${m[1]}::${name}` })
      }
    }
  }
  return [...twins]
}

// --- ledger: docs/rust-migration/ported-modules.md table rows ---------------------

function ledgerTsTokens(cell) {
  const tokens = []
  for (const m of cell.matchAll(/`([^`]+?\.ts)`/gu)) {
    const brace = m[1].match(/^(.*)\{([^}]+)\}(.*)$/u)
    if (brace === null) {
      tokens.push(m[1])
    } else {
      tokens.push(...brace[2].split(',').map((part) => `${brace[1]}${part.trim()}${brace[3]}`))
    }
  }
  return tokens
}

function ledgerRows() {
  const md = readRel('docs/rust-migration/ported-modules.md') ?? ''
  const rows = []
  let crate = null
  for (const line of md.split('\n')) {
    if (line.startsWith('## ')) {
      // Hyphenated crate names (orca-crash-recovery, orca-session-gc, …) must parse,
      // not just single-word ones — the E1 decision-core crates are all multi-word.
      crate = line.match(/`(orca-[a-z0-9-]+)`/u)?.[1] ?? null
      continue
    }
    if (crate === null || !line.startsWith('| `')) {
      continue
    }
    const cells = line.split('|').map((c) => c.trim())
    const rustMod = cells.at(1)?.match(/`([^`]+)`/u)?.[1]
    if (cells.length < 4 || rustMod === undefined || !/^[a-z0-9_]+$/u.test(rustMod)) {
      continue
    }
    rows.push({ crate, rustMod, sourceCell: cells.at(2) ?? '' })
  }
  return rows
}

// A missing ledger source may be a legitimately superseded (deleted) TS file: record
// every candidate path as a sentinel so an upstream merge resurrecting it fails loudly.
const ledgerCandidates = (tok) => [`src/shared/${tok}`, `src/main/${tok}`, `src/${tok}`]

// --- assembly ----------------------------------------------------------------------

function deriveMapping() {
  const unmapped = []
  const entries = new Map()
  const twinIndex = new Map() // rust file -> module id (for ledger-row merging)

  const registries = [
    {
      arms: 'rust/crates/orca-dispatch/src/modules/mod.rs',
      dir: 'rust/crates/orca-dispatch/src/modules'
    },
    {
      arms: 'rust/crates/orca-parity/src/modules/mod.rs',
      dir: 'rust/crates/orca-parity/src/modules'
    }
  ]
  for (const reg of registries) {
    for (const { module, rustMod } of registryArms(reg.arms)) {
      const dispatchTs = `tools/parity/dispatch/${module}.ts`
      const dispatchRs = `${reg.dir}/${rustMod}.rs`
      const { files, oracle } = parityTsSources(dispatchTs, unmapped, module)
      const twins = parityRustTwins(dispatchRs, unmapped, module)
      const vectors = `tools/parity/vectors/${module}.json`
      if (!existsRel(vectors)) {
        unmapped.push({ id: module, reason: `vector corpus missing: ${vectors}` })
      }
      const tsAbsent = new Set()
      if (oracle && files.length === 0 && !existsRel(`src/shared/${module}.ts`)) {
        tsAbsent.add(`src/shared/${module}.ts`) // superseded TS — reappearance is drift
      }
      if (
        oracle &&
        existsRel(`src/shared/${module}.ts`) &&
        !files.includes(`src/shared/${module}.ts`)
      ) {
        files.push(`src/shared/${module}.ts`) // types-only shell left behind by the cutover
      }
      entries.set(module, {
        id: module,
        origin: oracle ? 'parity (rust-sole: TS impl deleted, wasm oracle)' : 'parity',
        ts: files,
        tsAbsent,
        rust: twins,
        vectors: existsRel(vectors) ? vectors : null,
        dispatch: { ts: dispatchTs, rust: dispatchRs }
      })
      for (const t of twins) {
        twinIndex.set(t, module)
      }
    }
  }

  for (const { crate, rustMod, sourceCell } of ledgerRows()) {
    const rustFile = rustModuleFile(`rust/crates/${crate}`, rustMod)
    const id = `${crate}::${rustMod}`
    const tokens = ledgerTsTokens(sourceCell)
    if (rustFile === undefined) {
      unmapped.push({
        id,
        reason: `ledger rust module file not found under rust/crates/${crate}/src`
      })
      continue
    }
    if (tokens.length === 0) {
      unmapped.push({
        id,
        reason: 'ledger row has no TS source token (Rust-native or prose source)'
      })
      continue
    }
    const target = twinIndex.has(rustFile)
      ? entries.get(twinIndex.get(rustFile))
      : (entries.get(id) ??
        entries
          .set(id, {
            id,
            origin: 'ledger',
            ts: [],
            tsAbsent: new Set(),
            rust: [rustFile],
            vectors: null,
            dispatch: null
          })
          .get(id))
    for (const tok of tokens) {
      const hits = ledgerCandidates(tok).filter((c) => existsRel(c))
      if (hits.length > 1) {
        unmapped.push({ id, reason: `ambiguous TS resolution for \`${tok}\`: ${hits.join(' | ')}` })
      } else if (hits.length === 1) {
        if (!target.ts.includes(hits.at(0))) {
          target.ts.push(hits.at(0))
        }
      } else {
        for (const c of ledgerCandidates(tok)) {
          target.tsAbsent.add(c)
        }
      }
    }
  }

  // Hash everything; conflicted files are excluded (their hash would pin merge junk).
  const modules = []
  for (const e of [...entries.values()].sort((a, b) => (a.id < b.id ? -1 : 1))) {
    const hashFiles = (rels) => {
      const out = []
      for (const rel of rels.slice().sort()) {
        const content = readRel(rel)
        if (content === null) {
          unmapped.push({ id: e.id, reason: `mapped file vanished before hashing: ${rel}` })
        } else if (hasConflictMarkers(content)) {
          unmapped.push({
            id: e.id,
            reason: `merge-conflict markers in ${rel} — regenerate the manifest once the merge resolves`
          })
        } else {
          out.push({ path: rel, sha256: sha256(content) })
        }
      }
      return out.sort(byPath)
    }
    modules.push({
      id: e.id,
      origin: e.origin,
      label: `rust:[${e.rust.join(', ')}] <- ts:[${e.ts.length > 0 ? e.ts.join(', ') : '(none: rust-sole)'}]`,
      ts: hashFiles(e.ts),
      tsAbsent: [...e.tsAbsent].sort(),
      rust: hashFiles(e.rust),
      vectors: e.vectors,
      dispatch: e.dispatch
    })
  }
  const dedupedUnmapped = [
    ...new Map(unmapped.map((u) => [`${u.id} ${u.reason}`, u])).values()
  ].sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : a.reason < b.reason ? -1 : 1))
  const filesHashed = modules.reduce((n, m) => n + m.ts.length + m.rust.length, 0)
  return {
    version: 1,
    note: 'Generated by tools/port-provenance.mjs --generate. sha256 over LF-normalized content. Do not hand-edit.',
    stats: { modules: modules.length, filesHashed, unmapped: dedupedUnmapped.length },
    modules,
    unmapped: dedupedUnmapped
  }
}

// --- diff: derived mapping vs the committed manifest -------------------------------

function diffAgainstManifest(manifest, derived) {
  const drifts = []
  const manifestById = new Map(manifest.modules.map((m) => [m.id, m]))
  const derivedById = new Map(derived.modules.map((m) => [m.id, m]))
  const regen =
    'then regenerate the manifest (node tools/port-provenance.mjs --generate) and commit it'

  for (const [id, dm] of derivedById) {
    const mm = manifestById.get(id)
    if (mm === undefined) {
      drifts.push({
        kind: 'module-unregistered',
        module: id,
        action: `new ported module — ${regen}`
      })
      continue
    }
    const twins = dm.rust.map((r) => r.path)
    const vectors = dm.vectors ?? mm.vectors
    const pinnedTs = new Map(mm.ts.map((f) => [f.path, f.sha256]))
    for (const f of dm.ts) {
      const old = pinnedTs.get(f.path)
      pinnedTs.delete(f.path)
      if (old === undefined) {
        if (!(mm.tsAbsent ?? []).includes(f.path)) {
          drifts.push({
            kind: 'mapping-grew',
            module: id,
            file: f.path,
            action: `TS source newly mapped — review, ${regen}`
          })
        }
      } else if (old !== f.sha256) {
        drifts.push({
          kind: 'ts-drift',
          module: id,
          file: f.path,
          oldHash: old,
          newHash: f.sha256,
          rustTwins: twins,
          vectors,
          action: `TS reference changed — re-port into ${twins.join(', ')}; re-verify ${vectors ?? 'the module tests'}; ${regen}`
        })
      }
    }
    for (const [path, oldHash] of pinnedTs) {
      drifts.push({
        kind: existsRel(path) ? 'mapping-shrank' : 'ts-deleted',
        module: id,
        file: path,
        oldHash,
        rustTwins: twins,
        vectors,
        action: existsRel(path)
          ? `pinned TS file no longer derives from the registry — review, ${regen}`
          : `pinned TS source DELETED (superseded by the Rust port, or renamed upstream?) — review, ${regen}`
      })
    }
    for (const path of mm.tsAbsent ?? []) {
      if (existsRel(path)) {
        drifts.push({
          kind: 'ts-restored',
          module: id,
          file: path,
          rustTwins: twins,
          vectors,
          action: `superseded TS source REAPPEARED (upstream merge resurrection?) — re-delete in favor of ${twins.join(', ')} or re-port; re-verify ${vectors ?? 'the module tests'}; ${regen}`
        })
      }
    }
    const pinnedRust = new Map(mm.rust.map((f) => [f.path, f.sha256]))
    for (const f of dm.rust) {
      const old = pinnedRust.get(f.path)
      pinnedRust.delete(f.path)
      if (old === undefined) {
        drifts.push({
          kind: 'mapping-grew',
          module: id,
          file: f.path,
          action: `Rust twin newly mapped — review, ${regen}`
        })
      } else if (old !== f.sha256) {
        drifts.push({
          kind: 'rust-drift',
          module: id,
          file: f.path,
          oldHash: old,
          newHash: f.sha256,
          vectors,
          action: `Rust twin changed — re-verify ${vectors ?? 'the module tests'} (pnpm parity), ${regen}`
        })
      }
    }
    for (const [path, oldHash] of pinnedRust) {
      drifts.push({
        kind: 'rust-missing',
        module: id,
        file: path,
        oldHash,
        action: `pinned Rust twin vanished — review, ${regen}`
      })
    }
  }
  for (const id of manifestById.keys()) {
    if (!derivedById.has(id)) {
      drifts.push({
        kind: 'module-removed',
        module: id,
        action: `module left the registry/ledger — review, ${regen}`
      })
    }
  }
  const oldUn = new Set((manifest.unmapped ?? []).map((u) => `${u.id}: ${u.reason}`))
  const newUn = new Set(derived.unmapped.map((u) => `${u.id}: ${u.reason}`))
  const addedUn = [...newUn].filter((u) => !oldUn.has(u))
  const removedUn = [...oldUn].filter((u) => !newUn.has(u))
  if (addedUn.length > 0 || removedUn.length > 0) {
    drifts.push({
      kind: 'unmapped-changed',
      module: '(manifest)',
      added: addedUn,
      removed: removedUn,
      action: `the honestly-unmapped set changed — review, ${regen}`
    })
  }
  return drifts
}

// --- driver -------------------------------------------------------------------------

function main() {
  const args = new Set(process.argv.slice(2))
  const derived = deriveMapping()

  if (args.has('--generate')) {
    const out = join(overlay ?? repo, MANIFEST_REL)
    mkdirSync(dirname(out), { recursive: true })
    writeFileSync(out, `${JSON.stringify(derived, null, 2)}\n`)
    console.log(
      `port-provenance: wrote ${out} — ${derived.stats.modules} modules, ${derived.stats.filesHashed} files hashed, ${derived.stats.unmapped} unmapped`
    )
    return 0
  }

  const emit = (report) => {
    if (args.has('--json')) {
      console.log(JSON.stringify(report, null, 2))
    } else {
      console.log(`port-provenance: ${report.status} — ${JSON.stringify(report.stats)}`)
      for (const d of report.drifts) {
        console.log(`  [${d.kind}] ${d.module}${d.file === undefined ? '' : ` — ${d.file}`}`)
        if (d.oldHash !== undefined) {
          console.log(
            `      pinned ${d.oldHash.slice(0, 12)} -> current ${(d.newHash ?? '(gone)').slice(0, 12)}`
          )
        }
        console.log(`      ${d.action}`)
      }
    }
  }

  const manifestRaw = readRel(MANIFEST_REL)
  if (manifestRaw === null) {
    emit({
      status: 'no-manifest',
      stats: derived.stats,
      drifts: [
        {
          kind: 'no-manifest',
          module: '(manifest)',
          action: `no committed manifest at ${MANIFEST_REL} — generate it (node tools/port-provenance.mjs --generate) and commit it`
        }
      ]
    })
    return 2
  }
  const drifts = diffAgainstManifest(JSON.parse(manifestRaw), derived)
  emit({ status: drifts.length === 0 ? 'clean' : 'drift', stats: derived.stats, drifts })
  return drifts.length === 0 ? 0 : 2
}

process.exit(main())
