// F3 rung-1 autoformalize candidate scanner: AST-based discovery of exported
// TypeScript functions the ts2rust fuzz harness can drive, replacing hand-grep
// scouting. Emits tools/autoformalize-candidates.json and a ranked stdout table.
// Usage: node tools/autoformalize-candidate-scanner.mjs  (pnpm autoformalize:candidates)

import { readdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs'
import { createRequire } from 'node:module'
import { homedir } from 'node:os'
import { dirname, join, relative, sep } from 'node:path'
import {
  buildLocalTypeAliases,
  describeParamType,
  describeReturnType,
  inferReturnSpec
} from './autoformalize-signature-spec.mjs'
import { buildModuleContext, classifyFunctionBody } from './autoformalize-candidate-purity.mjs'

const REPO_ROOT = dirname(import.meta.dirname)
const SCAN_ROOTS = [
  join('src', 'shared'),
  join('src', 'main'),
  join('mobile', 'src'),
  join('src', 'renderer', 'src', 'lib')
]
const SKIP_DIRS = new Set([
  'node_modules',
  'dist',
  'build',
  'out',
  'coverage',
  '__tests__',
  '__mocks__',
  '.git'
])
const SKIP_FILE_RE = /\.(test|spec|bench|stories|d)\.[cm]?ts$/
const PORTED_CORPUS_DIR = join(homedir(), 'trust', 'tools', 'ts2rust', 'orca')
// Security/parsing-flavored names get ranked first inside each class: they are
// the highest-value formalization targets and the likeliest to have adversarial inputs.
const PRIORITY_NAME_RE = /secur|parse|valid|normaliz|sanitiz|escap|quot/i

// typescript@7 (native tsc) no longer ships the JS compiler API, so resolving
// 'typescript' from the repo root yields only a version shim. Probe the pnpm
// store for an older typescript build that still has createSourceFile — this
// adds no dependency, it only finds what pnpm already installed.
function loadTypeScriptModule() {
  const requireFromRepo = createRequire(join(REPO_ROOT, 'package.json'))
  const candidates = []
  try {
    candidates.push(requireFromRepo.resolve('typescript'))
  } catch {
    /* not hoisted at root — the store probe below still applies */
  }
  const pnpmDir = join(REPO_ROOT, 'node_modules', '.pnpm')
  if (existsSync(pnpmDir)) {
    for (const entry of readdirSync(pnpmDir)) {
      if (entry.startsWith('typescript@')) {
        candidates.push(join(pnpmDir, entry, 'node_modules', 'typescript', 'lib', 'typescript.js'))
      }
    }
  }
  for (const candidate of candidates) {
    try {
      const mod = requireFromRepo(candidate)
      if (typeof mod.createSourceFile === 'function' && mod.SyntaxKind) {
        return mod
      }
    } catch {
      /* try the next store entry */
    }
  }
  try {
    requireFromRepo.resolve('oxc-parser')
    throw new Error(
      'Only oxc-parser is available; the rung-1 scanner has a TypeScript-AST backend only. ' +
        'Install a typescript build with the JS compiler API (<=6.x) or add an oxc adapter.'
    )
  } catch (err) {
    if (err?.message?.includes('oxc adapter')) {
      throw err
    }
  }
  throw new Error(
    'No usable TypeScript compiler API found under node_modules (typescript@7 ships no JS API).'
  )
}

function* walkTsFiles(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name)
    if (entry.isDirectory()) {
      if (!SKIP_DIRS.has(entry.name)) {
        yield* walkTsFiles(full)
      }
    } else if (entry.name.endsWith('.ts') && !SKIP_FILE_RE.test(entry.name)) {
      yield full
    }
  }
}

// Exported function-likes: `export function f`, `export const f = (...) => ...`,
// and bottom-of-file `export { f }` clauses pointing at local function declarations.
function collectExportedFunctions(ts, sourceFile) {
  const out = []
  const exportClauseNames = new Set()
  for (const stmt of sourceFile.statements) {
    if (
      ts.isExportDeclaration(stmt) &&
      !stmt.moduleSpecifier &&
      stmt.exportClause &&
      ts.isNamedExports(stmt.exportClause)
    ) {
      for (const el of stmt.exportClause.elements) {
        exportClauseNames.add((el.propertyName ?? el.name).text)
      }
    }
  }
  const hasExportModifier = (stmt) =>
    stmt.modifiers?.some((m) => m.kind === ts.SyntaxKind.ExportKeyword)
  for (const stmt of sourceFile.statements) {
    if (ts.isFunctionDeclaration(stmt) && stmt.name && stmt.body) {
      if (hasExportModifier(stmt) || exportClauseNames.has(stmt.name.text)) {
        out.push({ name: stmt.name.text, fn: stmt })
      }
    } else if (ts.isVariableStatement(stmt) && hasExportModifier(stmt)) {
      for (const decl of stmt.declarationList.declarations) {
        const init = decl.initializer
        if (
          ts.isIdentifier(decl.name) &&
          init &&
          (ts.isArrowFunction(init) || ts.isFunctionExpression(init)) &&
          init.body
        ) {
          out.push({ name: decl.name.text, fn: init })
        }
      }
    }
  }
  return out
}

function qualifySignature(ts, fn, aliases) {
  const paramResults = []
  for (const param of fn.parameters) {
    const res = describeParamType(ts, param, aliases)
    if (!res.ok) {
      return null
    }
    const label = ts.isIdentifier(param.name) ? param.name.text : '{…}'
    paramResults.push({ ...res, label })
  }
  const ret = fn.type
    ? describeReturnType(ts, fn.type, aliases)
    : inferReturnSpec(ts, fn, aliases, paramResults)
  if (!ret.ok) {
    return null
  }
  return {
    argspec: `(${paramResults.map((p) => `${p.label}: ${p.spec}`).join(', ')}) -> ${ret.spec}`
  }
}

function loadPortedFunctionNames() {
  const names = new Set()
  if (!existsSync(PORTED_CORPUS_DIR)) {
    return { names, note: `ported-corpus dir not found: ${PORTED_CORPUS_DIR}` }
  }
  for (const entry of readdirSync(PORTED_CORPUS_DIR)) {
    if (!entry.endsWith('.ts')) {
      continue
    }
    const text = readFileSync(join(PORTED_CORPUS_DIR, entry), 'utf8')
    for (const match of text.matchAll(/export function ([A-Za-z0-9_$]+)/g)) {
      names.add(match[1])
    }
  }
  return { names, note: null }
}

const CLASS_RANK = { 'pure-self-contained': 0, 'needs-inline': 1, runtime: 3, impure: 4 }

function rankKey(entry) {
  let classRank = CLASS_RANK[entry.class] ?? 5
  if (entry.class === 'needs-inline' && entry.scope === 'cross-module') {
    classRank = 2
  }
  return [classRank, PRIORITY_NAME_RE.test(entry.fn) ? 0 : 1, entry.callees?.length ?? 0, entry.fn]
}

function compareRank(a, b) {
  const ka = rankKey(a)
  const kb = rankKey(b)
  for (let i = 0; i < ka.length; i++) {
    if (ka[i] !== kb[i]) {
      return ka[i] < kb[i] ? -1 : 1
    }
  }
  return 0
}

function truncate(text, width) {
  return text.length <= width ? text : `…${text.slice(-(width - 1))}`
}

function printTable(entries, counts) {
  const top = entries.slice(0, 40)
  const rows = top.map((e, i) => [
    String(i + 1),
    e.class + (e.scope ? `(${e.scope})` : ''),
    e.dup ? `${e.fn} [DUP]` : e.fn,
    truncate(e.file, 46),
    truncate(e['argspec-guess'], 44),
    truncate(e.reasons.slice(0, 2).join('; '), 40)
  ])
  const header = ['#', 'class', 'fn', 'file', 'argspec', 'notes']
  const widths = header.map((h, c) => Math.max(h.length, ...rows.map((r) => r[c].length)))
  const line = (cells) => cells.map((cell, c) => cell.padEnd(widths[c])).join('  ')
  console.log(line(header))
  console.log(widths.map((w) => '-'.repeat(w)).join('  '))
  for (const row of rows) {
    console.log(line(row))
  }
  console.log(
    `\n${counts.files} files scanned | ${counts.exported} exported fns | ${counts.qualified} drivable signatures | ` +
      `pure ${counts.pure} | needs-inline ${counts.needsInline} | excluded runtime ${counts.runtime} / impure ${counts.impure} | dups ${counts.dups}`
  )
}

function main() {
  const ts = loadTypeScriptModule()
  const entries = []
  const counts = {
    files: 0,
    exported: 0,
    qualified: 0,
    pure: 0,
    needsInline: 0,
    runtime: 0,
    impure: 0,
    dups: 0
  }
  for (const root of SCAN_ROOTS) {
    const absRoot = join(REPO_ROOT, root)
    if (!existsSync(absRoot)) {
      continue
    }
    for (const file of walkTsFiles(absRoot)) {
      counts.files++
      const text = readFileSync(file, 'utf8')
      if (!/export/.test(text)) {
        continue
      } // cheap pre-filter before a full parse
      const sourceFile = ts.createSourceFile(
        file,
        text,
        ts.ScriptTarget.Latest,
        true,
        ts.ScriptKind.TS
      )
      const exported = collectExportedFunctions(ts, sourceFile)
      if (exported.length === 0) {
        continue
      }
      counts.exported += exported.length
      const aliases = buildLocalTypeAliases(ts, sourceFile)
      let ctx = null
      for (const { name, fn } of exported) {
        const sig = qualifySignature(ts, fn, aliases)
        if (!sig) {
          continue
        }
        counts.qualified++
        ctx ??= buildModuleContext(ts, sourceFile)
        const verdict = classifyFunctionBody(ctx, fn)
        entries.push({
          file: relative(REPO_ROOT, file).split(sep).join('/'),
          fn: name,
          'argspec-guess': sig.argspec,
          class: verdict.cls,
          ...(verdict.scope ? { scope: verdict.scope } : {}),
          reasons: verdict.reasons,
          ...(verdict.cls === 'needs-inline' ? { callees: verdict.callees } : {}),
          dup: false
        })
      }
    }
  }
  const { names: portedNames, note: portedNote } = loadPortedFunctionNames()
  for (const entry of entries) {
    entry.dup = portedNames.has(entry.fn)
    if (entry.dup) {
      counts.dups++
    }
    if (entry.class === 'pure-self-contained') {
      counts.pure++
    } else if (entry.class === 'needs-inline') {
      counts.needsInline++
    } else if (entry.class === 'runtime') {
      counts.runtime++
    } else if (entry.class === 'impure') {
      counts.impure++
    }
  }
  entries.sort(compareRank)
  const outPath = join(REPO_ROOT, 'tools', 'autoformalize-candidates.json')
  writeFileSync(
    outPath,
    `${JSON.stringify(
      {
        generated: new Date().toISOString(),
        scanned: {
          roots: SCAN_ROOTS.map((r) => r.split(sep).join('/')),
          files: counts.files,
          exportedFunctions: counts.exported,
          drivableSignatures: counts.qualified,
          portedCorpus: portedNote ?? `${portedNames.size} exported fns in ${PORTED_CORPUS_DIR}`
        },
        candidates: entries
      },
      null,
      2
    )}\n`
  )
  printTable(entries, counts)
  console.log(`wrote ${relative(REPO_ROOT, outPath)} (${entries.length} entries)`)
  if (portedNote) {
    console.log(`note: ${portedNote} — dup flags all false`)
  }
}

main()
