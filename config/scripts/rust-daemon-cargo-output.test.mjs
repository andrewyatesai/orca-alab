import { spawn, spawnSync } from 'node:child_process'
import { chmodSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { delimiter, join, resolve } from 'node:path'
import { describe, expect, it } from 'vitest'
import {
  classifyRustDaemonCargoStderr,
  createCargoTemporalProofStderrFilter,
  createRustDaemonCargoStderrFilter
} from './rust-daemon-cargo-output.mjs'
import { hostTriple } from './rust-toolchain-lane.mjs'

// build-rust-daemon.mjs resolves its build tool through rust-toolchain-lane.mjs:
// `targo` (probed with `--unverified --version`) when the installed Trust
// toolchain ships the host std (`trustc --print target-libdir`), else the STOCK
// EXCEPTION lane via read-only `rustup` queries. The fakes answer all of it:
// `targo` passes the probe and otherwise IS the payload, `trustc` names a libdir
// holding a libstd rlib, and `rustup which cargo` names the same payload as `cargo`.
function writeFakeToolchain(fixtureDir, payload) {
  const fakeRustup = join(fixtureDir, 'rustup')
  writeFileSync(
    fakeRustup,
    `#!/usr/bin/env node
const [verb, sub] = process.argv.slice(2)
if (verb === 'toolchain') console.log('stable-fake (default)')
else if (verb === 'target') console.log(${JSON.stringify(hostTriple())})
else if (verb === 'which') console.log(sub === 'cargo' ? process.env.FAKE_CARGO : process.execPath)
`
  )
  chmodSync(fakeRustup, 0o755)
  const libdir = join(fixtureDir, 'sysroot', 'lib')
  mkdirSync(libdir, { recursive: true })
  writeFileSync(join(libdir, 'libstd-fake.rlib'), '')
  const fakeTrustc = join(fixtureDir, 'trustc')
  writeFileSync(fakeTrustc, `#!/usr/bin/env node\nconsole.log(${JSON.stringify(libdir)})\n`)
  chmodSync(fakeTrustc, 0o755)
  if (payload !== null) {
    const body = payload.replace(/^#!.*\n/, '')
    const fakeTargo = join(fixtureDir, 'targo')
    writeFileSync(
      fakeTargo,
      `#!/usr/bin/env node
if (process.argv.includes('--version')) { console.log('targo 0.0.0-fake'); process.exit(0) }
${body}`
    )
    writeFileSync(join(fixtureDir, 'cargo'), payload)
    chmodSync(fakeTargo, 0o755)
    chmodSync(join(fixtureDir, 'cargo'), 0o755)
  }
}

function fakeToolchainEnv(fixtureDir, fakeCargo) {
  const env = {
    ...process.env,
    PATH: `${fixtureDir}${delimiter}${process.env.PATH ?? ''}`,
    FAKE_CARGO: fakeCargo
  }
  delete env.CARGO
  delete env.RUSTC
  delete env.ORCA_STOCK_RUST_TOOLCHAIN
  return env
}

describe('classifyRustDaemonCargoStderr', () => {
  it('reclassifies the successful temporal proof receipt', () => {
    const result = classifyRustDaemonCargoStderr(
      '   Compiling aterm-grid v0.55.0\n' +
        'warning: aterm-grid@0.55.0: temporal gate ✓ ScrollbackOffloadWindow proven (Buggy=0) and non-vacuous (Buggy=1) by Trust ty\n' +
        '    Finished release profile\n'
    )

    expect(result).toEqual({
      stderr: '   Compiling aterm-grid v0.55.0\n    Finished release profile\n',
      proofReceipts: [
        'ScrollbackOffloadWindow proven (Buggy=0) and non-vacuous (Buggy=1) by Trust ty'
      ]
    })
  })

  it('preserves real warnings and partial final lines exactly', () => {
    const stderr = 'warning: unused variable `value`\nerror: build failed'
    expect(classifyRustDaemonCargoStderr(stderr)).toEqual({
      stderr,
      proofReceipts: []
    })
  })

  it.each([
    'warning: other-grid@0.55.0: temporal gate ✓ ScrollbackOffloadWindow proven (Buggy=0) and non-vacuous (Buggy=1) by Trust ty\n',
    'warning: aterm-grid@0.55.0: temporal gate ✗ ScrollbackOffloadWindow proven (Buggy=0) and non-vacuous (Buggy=1) by Trust ty\n',
    'warning: aterm-grid@0.55.0: temporal gate ✓ DifferentTheorem proven (Buggy=0) and non-vacuous (Buggy=1) by Trust ty\n',
    'warning: aterm-grid@0.55.0: temporal gate ✓ ScrollbackOffloadWindow proven (Buggy=0) and non-vacuous (Buggy=1) by Trust ty; unverified suffix\n'
  ])('preserves a near-match proof diagnostic byte-for-byte: %s', (stderr) => {
    expect(classifyRustDaemonCargoStderr(stderr)).toEqual({
      stderr,
      proofReceipts: []
    })
  })

  it('recognizes Cargo proof receipts with forced ANSI styling', () => {
    const stderr =
      '\x1b[1m\x1b[33mwarning\x1b[0m\x1b[1m: aterm-grid@0.55.0: temporal gate ✓ ScrollbackOffloadWindow proven (Buggy=0) and non-vacuous (Buggy=1) by Trust ty\x1b[0m\n'

    expect(classifyRustDaemonCargoStderr(stderr)).toEqual({
      stderr: '',
      proofReceipts: [
        'ScrollbackOffloadWindow proven (Buggy=0) and non-vacuous (Buggy=1) by Trust ty'
      ]
    })
  })

  it('streams output and handles proof receipts split across chunks', async () => {
    const filter = createRustDaemonCargoStderrFilter()
    const output = []
    filter.on('data', (chunk) => output.push(chunk.toString()))

    filter.write('   Compiling aterm-grid v0.55.0\nwarning: aterm-grid@0.55.0: temporal ')
    filter.write(
      'gate ✓ ScrollbackOffloadWindow proven (Buggy=0) and non-vacuous (Buggy=1) by Trust ty\r\nwarning: real'
    )
    filter.end(' warning')

    await new Promise((resolve, reject) => {
      filter.once('end', resolve)
      filter.once('error', reject)
      filter.resume()
    })

    expect(output.join('')).toBe(
      '   Compiling aterm-grid v0.55.0\n' +
        '[build-rust-daemon] verified ScrollbackOffloadWindow proven (Buggy=0) and non-vacuous (Buggy=1) by Trust ty\r\n' +
        'warning: real warning'
    )
  })

  it('preserves a bare carriage return on a final proof line', async () => {
    const filter = createRustDaemonCargoStderrFilter()
    const output = []
    filter.on('data', (chunk) => output.push(chunk.toString()))

    filter.end(
      'warning: aterm-grid@0.55.0: temporal gate ✓ ScrollbackOffloadWindow proven (Buggy=0) and non-vacuous (Buggy=1) by Trust ty\r'
    )
    await new Promise((resolve, reject) => {
      filter.once('end', resolve)
      filter.once('error', reject)
      filter.resume()
    })

    expect(output.join('')).toBe(
      '[build-rust-daemon] verified ScrollbackOffloadWindow proven (Buggy=0) and non-vacuous (Buggy=1) by Trust ty\r'
    )
  })

  it.each(['terminal-addon', 'aterm-wasm'])('uses the %s caller label', async (label) => {
    const filter = createCargoTemporalProofStderrFilter(label)
    const output = []
    filter.on('data', (chunk) => output.push(chunk.toString()))

    filter.end(
      'warning: aterm-grid@999.0.0: temporal gate ✓ ScrollbackOffloadWindow proven (Buggy=0) and non-vacuous (Buggy=1) by Trust ty\n'
    )
    await new Promise((resolve, reject) => {
      filter.once('end', resolve)
      filter.once('error', reject)
      filter.resume()
    })

    expect(output.join('')).toBe(
      `[${label}] verified ScrollbackOffloadWindow proven (Buggy=0) and non-vacuous (Buggy=1) by Trust ty\n`
    )
  })

  const itOnPosix = process.platform === 'win32' ? it.skip : it
  itOnPosix('drains streamed Cargo diagnostics before a failed build exits', () => {
    const fixtureDir = mkdtempSync(join(tmpdir(), 'orca-rust-stream-'))
    const fakeCargo = join(fixtureDir, 'cargo')
    const cargoPayload = `warning: ${'x'.repeat(2 * 1024 * 1024)} END-OF-CARGO-STDERR\n`

    try {
      writeFakeToolchain(
        fixtureDir,
        `#!/usr/bin/env node\nprocess.stderr.write(${JSON.stringify(cargoPayload)})\nprocess.exitCode = 7\n`
      )

      const result = spawnSync(
        process.execPath,
        [resolve(import.meta.dirname, 'build-rust-daemon.mjs')],
        {
          encoding: 'utf8',
          maxBuffer: 4 * 1024 * 1024,
          env: fakeToolchainEnv(fixtureDir, fakeCargo)
        }
      )

      expect(result.status).toBe(7)
      expect(result.signal).toBeNull()
      expect(result.stderr).toBe(`${cargoPayload}[build-rust-daemon] cargo build failed (exit 7)\n`)
    } finally {
      rmSync(fixtureDir, { recursive: true, force: true })
    }
  })

  itOnPosix('reports Cargo spawn failures without an unlabelled stack trace', () => {
    const fixtureDir = mkdtempSync(join(tmpdir(), 'orca-rust-spawn-'))
    const missingCargo = join(fixtureDir, 'missing-cargo')

    try {
      writeFakeToolchain(fixtureDir, null)
      const result = spawnSync(
        process.execPath,
        [resolve(import.meta.dirname, 'build-rust-daemon.mjs')],
        {
          encoding: 'utf8',
          // CARGO names the build tool on a Trust host; the fake rustup names it
          // on a STOCK EXCEPTION host. Both point at nothing.
          env: { ...fakeToolchainEnv(fixtureDir, missingCargo), CARGO: missingCargo }
        }
      )

      expect(result.status).toBe(1)
      expect(result.signal).toBeNull()
      expect(result.stderr).toContain('[build-rust-daemon] could not ')
      expect(result.stderr).toContain('ENOENT')
      expect(result.stderr).not.toContain('at runCargoBuild')
    } finally {
      rmSync(fixtureDir, { recursive: true, force: true })
    }
  })

  itOnPosix(
    'mirrors job control and forwards cancellation to the complete Cargo process group',
    async () => {
      const fixtureDir = mkdtempSync(join(tmpdir(), 'orca-rust-signal-'))
      const fakeCargo = join(fixtureDir, 'cargo')
      const armedMarker = join(fixtureDir, 'armed-rustc')
      const continuedMarker = join(fixtureDir, 'continued-rustc')
      const orphanMarker = join(fixtureDir, 'orphaned-rustc')
      let wrapper
      let cargoPid = null

      try {
        writeFakeToolchain(
          fixtureDir,
          `#!/usr/bin/env node
const { spawn } = require('node:child_process')
const { existsSync } = require('node:fs')
spawn(process.execPath, ['-e', ${JSON.stringify(
            `const { writeFileSync } = require('node:fs'); writeFileSync(${JSON.stringify(armedMarker)}, 'armed'); setTimeout(() => writeFileSync(${JSON.stringify(continuedMarker)}, 'continued'), 400)`
          )}], { stdio: 'ignore' })
spawn(process.execPath, ['-e', ${JSON.stringify(
            `setTimeout(() => require('node:fs').writeFileSync(${JSON.stringify(orphanMarker)}, 'orphaned'), 1500)`
          )}], { stdio: 'ignore' })
const readyTimer = setInterval(() => {
  if (existsSync(${JSON.stringify(armedMarker)})) {
    clearInterval(readyTimer)
    process.stderr.write(\`cargo-ready \${process.pid}\\n\`)
  }
}, 10)
setInterval(() => {}, 1000)
`
        )

        wrapper = spawn(process.execPath, [resolve(import.meta.dirname, 'build-rust-daemon.mjs')], {
          env: fakeToolchainEnv(fixtureDir, fakeCargo),
          stdio: ['ignore', 'ignore', 'pipe']
        })
        wrapper.stderr.setEncoding('utf8')
        let stderr = ''
        const closeResult = new Promise((resolveClose, rejectClose) => {
          wrapper.once('error', rejectClose)
          wrapper.once('close', (status, signal) => resolveClose({ status, signal }))
        })
        await new Promise((resolveReady, rejectReady) => {
          // Why 15s, not 2s: reaching "cargo-ready" costs SEVEN Node startups
          // (this wrapper -> build-rust-daemon -> the targo identity probe ->
          // the trustc libdir probe -> fake cargo -> its two children)
          // plus marker-file polling. That is process-startup latency, which this
          // test does not assert — it asserts SIGTSTP/SIGCONT/SIGQUIT job control.
          // A 2s budget held when run alone and flaked inside the full parallel
          // suite, failing as "fake Cargo did not start" rather than anything
          // about job control. 15s is generous enough never to be the thing under
          // test, and stays inside the 30s testTimeout so THIS message survives
          // rather than being masked by a bare vitest timeout.
          const timeout = setTimeout(
            () => rejectReady(new Error(`fake Cargo did not start; stderr=${stderr}`)),
            15_000
          )
          wrapper.stderr.on('data', (chunk) => {
            stderr += chunk
            const match = /cargo-ready (\d+)/.exec(stderr)
            if (match) {
              cargoPid = Number(match[1])
              clearTimeout(timeout)
              resolveReady()
            }
          })
        })

        wrapper.kill('SIGTSTP')
        await new Promise((resolveWait) => setTimeout(resolveWait, 700))
        expect(existsSync(continuedMarker)).toBe(false)

        wrapper.kill('SIGCONT')
        await new Promise((resolveWait) => setTimeout(resolveWait, 300))
        expect(existsSync(continuedMarker)).toBe(true)

        wrapper.kill('SIGQUIT')
        const result = await Promise.race([
          closeResult,
          new Promise((_, rejectClose) =>
            setTimeout(
              () => rejectClose(new Error('build wrapper did not exit after SIGQUIT')),
              2_000
            )
          )
        ])
        await new Promise((resolveWait) => setTimeout(resolveWait, 700))

        expect(result).toEqual({ status: 131, signal: null })
        expect(stderr).toContain('[build-rust-daemon] cargo build terminated by SIGQUIT')
        expect(existsSync(orphanMarker)).toBe(false)
      } finally {
        if (wrapper && wrapper.exitCode === null && wrapper.signalCode === null) {
          wrapper.kill('SIGKILL')
        }
        if (cargoPid) {
          try {
            process.kill(-cargoPid, 'SIGKILL')
          } catch {}
        }
        rmSync(fixtureDir, { recursive: true, force: true })
      }
    },
    6_000
  )
})
