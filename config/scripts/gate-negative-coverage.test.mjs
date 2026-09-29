import { spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import {
  GATE_COVERAGE_LEDGER_ENV,
  GATE_COVERAGE_TAG,
  assertGateAccepts,
  assertGateRejects
} from './assert-gate-rejects-violation.mjs'

const REPO_ROOT = path.resolve(import.meta.dirname, '..', '..')
const GATE_SCRIPT_NAME = /^(?:check|verify|lint):/
/**
 * Gates that ARE enforcement but are raw linter invocations rather than
 * `node config/scripts/*.mjs`, so `gateScripts()` cannot name a script for them.
 *
 * They were invisible to this file until now — the scope was `check:`/`verify:`
 * only — even though `lint:switch-exhaustiveness` sits inside the `lint` chain
 * and blocks like any other gate. Listing them here keeps them covered without
 * weakening the rule below that a SCRIPT-backed gate must name its script.
 *
 * The value is the identity its negative test passes to `recordLinterRejection`,
 * so these claims are still settled by an observed non-zero exit, not by a tag.
 */
const LINTER_BACKED_GATES = new Map([
  ['lint:switch-exhaustiveness', 'oxlint:switch-exhaustiveness']
])
/**
 * Reporting variants, not gates. Established by running them, not by their names:
 * every rule in `config/oxlint-react-doctor.json` is severity `warn`, and oxlint
 * exits 0 on warnings, so neither react-doctor `lint:` script can fail. That is
 * deliberate — react-doctor's enforcement happens on ADDED LINES so pre-existing
 * debt does not block routine work, and two gates already do it properly:
 *   - `check:react-doctor:changed`  -> react-doctor CLI, `--scope lines --blocking error`
 *   - `check:code-quality:changed`  -> runs the SAME oxlint react-doctor config as
 *     one of its three scans, filtered through overlapsAddedLines()
 * Both are covered by negative tests here.
 *
 * Verified the alternative is wrong before settling on this: adding
 * `--deny-warnings` to lint-react-doctor-changed.mjs makes it reject a planted
 * violation, but it lints whole FILES rather than added lines — touching
 * BrowserPane.tsx with a one-line comment then fails on 3 pre-existing findings
 * unrelated to the edit. Enforcing there would block routine work on legacy files
 * while adding nothing the two gates above do not already catch.
 */
const REPORTING_VARIANTS = new Set(['lint', 'lint:react-doctor:changed'])
// Every gate is a config/scripts entry point, so its negative test belongs beside it.
const COVERAGE_ROOT = path.join(REPO_ROOT, 'config', 'scripts')
const VITEST_CLI = path.join(REPO_ROOT, 'node_modules', 'vitest', 'vitest.mjs')
const VITEST_CONFIG = path.join(REPO_ROOT, 'config', 'vitest.config.ts')
const GATE_COMMAND_SCRIPT = /config[/\\]scripts[/\\](\S+\.mjs)/
const TAG_CLAIM = new RegExp(`${GATE_COVERAGE_TAG}\\s+([\\w:.-]+)`, 'g')

/**
 * Gates whose failure path nothing has ever executed, and why.
 *
 * A gate here can be silently inert — parsing nothing, scanning an empty set, or
 * matching a pattern that can never hit — and every run still reports success. The
 * list only shrinks: pairing a gate with a negative test and deleting its row is the
 * whole job, and this test fails if a row outlives the gap it describes.
 */
export const GATES_MISSING_A_NEGATIVE_TEST = new Map([
  // Rust-cutover campaign gates, newer than the sweep that emptied this list.
  ['verify:rust', 'no test of any kind'],
  ['check:cutover-census', 'no test of any kind']
])

/** Gate name -> the script package.json runs, which is what a claim has to execute. */
function gateScripts() {
  const { scripts } = JSON.parse(readFileSync(path.join(REPO_ROOT, 'package.json'), 'utf8'))
  return new Map(
    Object.entries(scripts ?? {})
      .filter(([name]) => GATE_SCRIPT_NAME.test(name) && !REPORTING_VARIANTS.has(name))
      .map(([name, command]) => [
        name,
        LINTER_BACKED_GATES.get(name) ?? GATE_COMMAND_SCRIPT.exec(command)?.[1] ?? null
      ])
  )
}

function testFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const entryPath = path.join(directory, entry.name)
    if (entry.isDirectory()) {
      return testFiles(entryPath)
    }
    return /\.test\.(?:mjs|ts)$/.test(entry.name) ? [entryPath] : []
  })
}

function relative(file) {
  return path.relative(REPO_ROOT, file).split(path.sep).join('/')
}

/** Files claiming a gate, minus this one — a self-claim would credit its own tag. */
function taggedTestFiles() {
  const tagged = new Map()
  for (const file of testFiles(COVERAGE_ROOT)) {
    if (file === import.meta.filename) {
      continue
    }
    const gates = [
      ...new Set([...readFileSync(file, 'utf8').matchAll(TAG_CLAIM)].map(([, gate]) => gate))
    ]
    if (gates.length > 0) {
      tagged.set(file, gates)
    }
  }
  return tagged
}

// A nested run is the only thing that can tell a live assertion from a commented-out
// one: the claim is settled by the exits the tagged file provoked, not by its text.
function runTaggedFile(file, ledger) {
  const result = spawnSync(
    process.execPath,
    // One worker: this runs inside a worker of the outer suite, whose load-sensitive
    // sibling files should not lose CPU to a nested pool sized for a whole run.
    [VITEST_CLI, 'run', '--config', VITEST_CONFIG, '--reporter=dot', '--maxWorkers=1', file],
    {
      cwd: REPO_ROOT,
      encoding: 'utf8',
      env: { ...process.env, [GATE_COVERAGE_LEDGER_ENV]: ledger, FORCE_COLOR: '0' }
    }
  )
  if (result.error) {
    throw new Error(`Could not run ${relative(file)}: ${result.error.message}`)
  }
  return { status: result.status, output: `${result.stdout ?? ''}${result.stderr ?? ''}` }
}

function ledgerRejections(ledger) {
  if (!existsSync(ledger)) {
    return []
  }
  return readFileSync(ledger, 'utf8')
    .split('\n')
    .filter(Boolean)
    .map((line) => JSON.parse(line))
    .filter((entry) => entry.status !== 0)
}

function proveClaims(ledgerDir) {
  const scripts = gateScripts()
  const claims = []
  const runs = []
  let rejections = 0
  for (const [file, gates] of taggedTestFiles()) {
    const ledger = path.join(ledgerDir, `${path.basename(file)}.jsonl`)
    const run = runTaggedFile(file, ledger)
    runs.push({ file: relative(file), status: run.status, output: run.output })
    const executed = new Set(ledgerRejections(ledger).map((entry) => entry.script))
    rejections += executed.size
    for (const gate of gates) {
      const script = scripts.get(gate)
      claims.push({
        file: relative(file),
        gate,
        proven: run.status === 0 && Boolean(script) && executed.has(path.basename(script))
      })
    }
  }
  return { scripts, claims, runs, rejections }
}

let ledgerDir = null
let evidence = null

describe('the rejection helpers stay synchronous', () => {
  /**
   * Load-bearing, and not obvious: every negative test in config/scripts calls
   * `assertGateAccepts(...)` / `assertGateRejects(...)` WITHOUT `await`, which is
   * correct only because both are synchronous (spawnSync + throw).
   *
   * Make either one async and every one of those call sites becomes an unawaited
   * promise: the throws turn into unhandled rejections, and every negative test
   * in the repo passes unconditionally while proving nothing. That is the exact
   * failure this whole file exists to prevent, so it is asserted rather than
   * left as a comment for the refactor to ignore.
   */
  it('are not async, because every negative test calls them unawaited', () => {
    expect(assertGateAccepts.constructor.name).toBe('Function')
    expect(assertGateRejects.constructor.name).toBe('Function')
  })

  it('throws rather than resolving when a gate accepts a planted violation', () => {
    // Proves the mechanism the point above depends on: the failure is a SYNCHRONOUS
    // throw, so an unawaited call still fails its test today.
    expect(() =>
      assertGateRejects({
        script: path.join(REPO_ROOT, 'config', 'scripts', 'noop-always-exits-zero.mjs'),
        cwd: REPO_ROOT,
        violation: 'a gate that cannot fail',
        expectMessage: 'unreachable'
      })
    ).toThrow()
  })
})

describe('every package.json gate proves it can fail', () => {
  beforeAll(() => {
    ledgerDir = mkdtempSync(path.join(tmpdir(), 'orca-gate-coverage-'))
    evidence = proveClaims(ledgerDir)
  })

  afterAll(() => {
    if (ledgerDir) {
      rmSync(ledgerDir, { recursive: true, force: true })
    }
    ledgerDir = null
  })

  function coveredGates() {
    const covered = new Map()
    for (const claim of evidence.claims.filter(({ proven }) => proven)) {
      covered.set(claim.gate, [...(covered.get(claim.gate) ?? []), claim.file])
    }
    return covered
  }

  it('enumerates gates, claims and observed rejections from disk, not from a hand list', () => {
    expect(evidence.scripts.size).toBeGreaterThan(0)
    expect(evidence.runs.length).toBeGreaterThan(0)
    expect(evidence.claims.length).toBeGreaterThan(0)
    // Zero recorded rejections means the ledger stopped being written; the checks below
    // would then "pass" by declaring every gate known debt.
    expect(evidence.rejections).toBeGreaterThan(0)
    expect(coveredGates().size).toBeGreaterThan(0)
    // A gate whose command stops being a config/scripts entry point can never be proven.
    expect([...evidence.scripts].filter(([, script]) => !script).map(([gate]) => gate)).toEqual([])
    // A tag for a gate that no longer exists silently stops covering anything.
    expect(
      evidence.claims
        .filter(({ gate }) => !evidence.scripts.has(gate))
        .map(({ file, gate }) => `${file} claims ${gate}`)
    ).toEqual([])
  })

  it('runs every tagged negative test and requires it to pass', () => {
    expect(
      evidence.runs
        .filter(({ status }) => status !== 0)
        .map(({ file, status, output }) => `${file} exited ${status}:\n${output.slice(-2000)}`)
    ).toEqual([])
  })

  it('settles each tag by an exit it watched that gate take, not by the text around it', () => {
    expect(
      evidence.claims
        .filter(({ proven }) => !proven)
        .map(
          ({ file, gate }) =>
            `${file} claims ${gate} but never ran ${evidence.scripts.get(gate) ?? '(no script)'} to a non-zero exit`
        )
    ).toEqual([])
  })

  it('pairs each gate with a test that plants a violation and asserts a non-zero exit', () => {
    const covered = coveredGates()
    const uncovered = [...evidence.scripts.keys()].filter(
      (gate) => !covered.has(gate) && !GATES_MISSING_A_NEGATIVE_TEST.has(gate)
    )

    expect(uncovered).toEqual([])
  })

  it('keeps the acknowledged-debt list accurate as gates gain coverage', () => {
    const covered = coveredGates()
    const acknowledged = [...GATES_MISSING_A_NEGATIVE_TEST.keys()]

    expect(acknowledged.filter((gate) => !evidence.scripts.has(gate))).toEqual([])
    expect(acknowledged.filter((gate) => covered.has(gate))).toEqual([])
  })

  it('reports a gate that accepts a planted violation', () => {
    // The harness every claim above depends on: prove it fails a passing gate rather
    // than trusting that it would.
    expect(() =>
      assertGateRejects({
        script: path.join(COVERAGE_ROOT, 'assert-gate-rejects-violation.mjs'),
        cwd: REPO_ROOT,
        violation: 'a script that exits 0',
        expectMessage: 'never reached'
      })
    ).toThrow('Gate accepted a planted violation')
  })
})
