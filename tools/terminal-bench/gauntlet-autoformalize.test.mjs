import { spawnSync } from 'node:child_process'
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { EXIT } from './gauntlet-exit-code.mjs'

const STATIC_REFUTATION = '[witness 1: STATIC PROOF]   Verdict=REFUTED  (0 proved, 1 failed)'
const DIFF_REFUTATION = '[witness 2: DIFFERENTIAL]   1 / 10 divergences'
const TRUSTED = 'VERDICT: TRUSTED — both witnesses passed'
const NOT_TRUSTED = 'VERDICT: NOT TRUSTED — repair signal:'
const BASELINE = { minTrusted: 2, soundnessControls: 1 }
const HEALTHY = [{ name: 'k1' }, { name: 'k2' }, { name: 'ctl_bug', refute: true }]

// Only the external harness is a double; every verdict comes through the production CLI.
const DRIVER = `import { readFileSync } from 'node:fs'
const line = readFileSync(process.argv[5], 'utf8').split('\\n')[0]
const { output, code, signal } = JSON.parse(line.slice('// OUTCOME '.length))
console.log(output)
if (signal) process.kill(process.pid, signal)
else process.exit(code)
`

function runAutoformalize(kernels, ratchet = BASELINE) {
  const root = mkdtempSync(join(tmpdir(), 'orca-gauntlet-autoformalize-'))
  const here = join(root, 'tools', 'terminal-bench')
  const trustRoot = join(root, 'trust')
  const ts2rust = join(trustRoot, 'tools', 'ts2rust')
  const trustc = join(trustRoot, 'build', 'host', 'stage2', 'bin', 'trustc')
  try {
    mkdirSync(here, { recursive: true })
    // Copy the unmodified CLI and its modules so tests never rewrite a live ratchet/report.
    for (const file of readdirSync(import.meta.dirname)) {
      if (
        (file.startsWith('gauntlet') || file === 'rust-type-to-argspec.mjs') &&
        file.endsWith('.mjs') &&
        !file.endsWith('.test.mjs')
      ) {
        copyFileSync(join(import.meta.dirname, file), join(here, file))
      }
    }
    const corpusModule = join(root, 'tools', 'aterm-vs-xterm', 'corpus-bytes.mjs')
    mkdirSync(dirname(corpusModule), { recursive: true })
    copyFileSync(
      join(import.meta.dirname, '..', 'aterm-vs-xterm', 'corpus-bytes.mjs'),
      corpusModule
    )
    mkdirSync(join(ts2rust, 'orca'), { recursive: true })
    mkdirSync(dirname(trustc), { recursive: true })
    writeFileSync(join(ts2rust, 'autoformalize.mjs'), DRIVER)
    writeFileSync(trustc, '')
    if (ratchet !== null) {
      writeFileSync(
        join(here, 'autoformalize-ratchet.json'),
        typeof ratchet === 'string' ? ratchet : JSON.stringify(ratchet)
      )
    }
    for (const { name, refute, output, code, signal, signature } of kernels) {
      const outcome = {
        output: output ?? (refute ? `${STATIC_REFUTATION}\n${NOT_TRUSTED}` : TRUSTED),
        code: code ?? (refute ? 1 : 0),
        signal
      }
      writeFileSync(join(ts2rust, 'orca', `${name}.ts`), 'export function k(x) { return x }\n')
      writeFileSync(
        join(ts2rust, 'orca', `${name}.rs`),
        `// OUTCOME ${JSON.stringify(outcome)}\n${signature ?? 'pub fn k(x: u32) -> u32 { x }'}\n`
      )
    }
    const run = spawnSync(process.execPath, [join(here, 'gauntlet.mjs'), 'autoformalize'], {
      cwd: root,
      encoding: 'utf8',
      timeout: 10_000,
      env: { ...process.env, TRUST_REPO: trustRoot, TRUSTC: trustc }
    })
    expect(run.error, run.stderr).toBeUndefined()
    const report = JSON.parse(readFileSync(join(here, '.gauntlet-report.json'), 'utf8'))
    expect(report.exit).toBe(run.status)
    return { code: run.status, ...report.results.autoformalize }
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
}

describe('autoformalize production CLI ratchets', () => {
  it('passes a healthy measured corpus and actual negative-control refutation', () => {
    const result = runAutoformalize(HEALTHY)
    expect(result).toMatchObject({
      code: EXIT.pass,
      status: 'PASS',
      metrics: { trusted: 2, total: 3, controls: 1, refutedControls: 1 }
    })
  })

  it('accepts differential refutation as negative-control evidence', () => {
    const result = runAutoformalize([
      ...HEALTHY.slice(0, 2),
      { name: 'ctl_naive', code: 1, output: `${DIFF_REFUTATION}\n${NOT_TRUSTED}` }
    ])
    expect(result.code).toBe(EXIT.pass)
    expect(result.metrics.refutedControls).toBe(1)
  })

  it('fails when a ratcheted corpus vanishes or all signatures are declined', () => {
    for (const kernels of [
      [],
      [{ name: 'unsupported', signature: 'pub fn k(x: Unsupported) {}' }]
    ]) {
      const result = runAutoformalize(kernels)
      expect(result.code).toBe(EXIT.fail)
      expect(result.detail).toMatch(/ratcheted corpus is GONE/u)
    }
  })

  it('fails when a negative control vanishes or becomes trusted', () => {
    const missing = runAutoformalize(HEALTHY.slice(0, 2))
    expect(missing.code).toBe(EXIT.fail)
    expect(missing.detail).toMatch(/soundness controls SHRANK.*vacuous/u)
    const trusted = runAutoformalize([...HEALTHY.slice(0, 2), { name: 'ctl_bug' }])
    expect(trusted.code).toBe(EXIT.fail)
    expect(trusted.detail).toMatch(/SOUNDNESS BREAK/u)
  })

  it('fails when a formerly trusted kernel is refuted', () => {
    const result = runAutoformalize([{ name: 'k1', refute: true }, ...HEALTHY.slice(1)])
    expect(result.code).toBe(EXIT.fail)
    expect(result.detail).toMatch(/TRUSTED count regressed: 1 < baseline 2/u)
  })

  it('reviews faithful misses even when the trusted baseline is still met', () => {
    const result = runAutoformalize([
      ...HEALTHY,
      { name: 'new_port', code: 1, output: NOT_TRUSTED }
    ])
    expect(result.code).toBe(EXIT.review)
    expect(result.detail).toMatch(/1 faithful port\(s\) not proved/u)
  })

  it.each([
    null,
    {},
    { minTrusted: 2 },
    { soundnessControls: 1 },
    { ...BASELINE, minTrusted: -1 },
    { ...BASELINE, minTrusted: 1.5 },
    { ...BASELINE, minTrusted: '2' },
    { ...BASELINE, soundnessControls: 0 },
    { ...BASELINE, soundnessControls: -1 },
    { ...BASELINE, soundnessControls: 1.5 },
    { ...BASELINE, soundnessControls: '1' },
    '{',
    'null',
    '[]'
  ])('cannot pass with an absent or invalid ratchet: %j', (ratchet) => {
    const result = runAutoformalize(HEALTHY, ratchet)
    expect(result.code).toBe(EXIT.review)
    expect(result.detail).toMatch(/NOT ratcheted/u)
  })

  it('skips only an unclaimed empty corpus; an invalid baseline cannot erase a claim', () => {
    expect(runAutoformalize([], null).code).toBe(EXIT.nothingProven)
    for (const ratchet of ['{', 'null', {}, { minTrusted: 0 }]) {
      expect(runAutoformalize([], ratchet).code).toBe(EXIT.fail)
    }
  })

  it('retains generic/trailing-comma discovery and treats suffixes, not substrings, as controls', () => {
    const result = runAutoformalize([
      { name: 'k_bug_toobig', signature: "pub fn k<'a>(x: u32,\n) -> u32 { x }" },
      ...HEALTHY.slice(1)
    ])
    expect(result.code).toBe(EXIT.pass)
    expect(result.metrics.controls).toBe(1)
  })
})

describe('autoformalize production CLI evidence integrity', () => {
  it.each([
    { code: 0, output: '' },
    { signal: 'SIGTERM', output: TRUSTED },
    { code: 1, output: TRUSTED },
    { code: 2, output: TRUSTED },
    { code: 0, output: `${TRUSTED}\n${NOT_TRUSTED}` },
    { code: 0, output: `${TRUSTED}\n${TRUSTED}` },
    { code: 0, output: NOT_TRUSTED }
  ])('does not count inconsistent positive evidence as trusted: %j', (outcome) => {
    const result = runAutoformalize([{ name: 'k1', ...outcome }, ...HEALTHY.slice(1)])
    expect(result.code).toBe(EXIT.fail)
    expect(result.metrics.trusted).toBe(1)
    expect(result.rows.find((row) => row.name === 'k1').verdict).toBe('INCOMPLETE')
  })

  it.each([
    { code: 1, output: 'Error: harness failed' },
    { signal: 'SIGTERM', output: `${STATIC_REFUTATION}\n${NOT_TRUSTED}` },
    { code: 1, output: `${NOT_TRUSTED}\n  · differential failed to build/run` },
    { code: 1, output: `[witness 1: STATIC PROOF] Verdict=INCOMPLETE\n${NOT_TRUSTED}` },
    { code: 1, output: `[witness 2: DIFFERENTIAL] 0 / 10 divergences\n${NOT_TRUSTED}` },
    { code: 1, output: `${STATIC_REFUTATION}\n${TRUSTED}` },
    { code: 2, output: `${STATIC_REFUTATION}\n${NOT_TRUSTED}` },
    { code: 0, output: `${STATIC_REFUTATION}\n${NOT_TRUSTED}` }
  ])('does not launder incomplete/failed control runs as refutations: %j', (outcome) => {
    const result = runAutoformalize([...HEALTHY.slice(0, 2), { name: 'ctl_bug', ...outcome }])
    expect(result.code).toBe(EXIT.fail)
    expect(result.metrics.refutedControls).toBe(0)
    expect(result.detail).toMatch(/NOT REFUTED/u)
  })
})
