import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { delimiter, dirname, join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  RustToolchainError,
  TRUST_REQUIRED_TRIPLES,
  cargoInvocation,
  hostTriple,
  probeTrustTriple,
  resolveTargo,
  stockExceptionToolchain
} from './rust-toolchain-lane.mjs'

// Why: these are the migration's policy invariants (owner directive 2026-09-28):
// no stock fallback where Trust is required, the lane follows what the INSTALLED
// Trust toolchain ships, rustup never installs anything, and upstream cargo cannot
// pose as targo. They are pinned here with fake tools on PATH, not only indirectly
// through build-rust-daemon's fakes.

const itOnPosix = process.platform === 'win32' ? it.skip : it
// A triple outside TRUST_REQUIRED_TRIPLES, served or not per test.
const OTHER = 'x86_64-unknown-linux-gnu'
const WASM = 'wasm32-unknown-unknown'

let fixtureDir

function writeTool(name, lines) {
  const file = join(fixtureDir, name)
  writeFileSync(file, ['#!/usr/bin/env node', ...lines, ''].join('\n'))
  chmodSync(file, 0o755)
  return file
}

// Fake targo, trustc and rustup. Each triple in `served` gets a libstd rlib in
// the fake Trust sysroot; the fake rustup logs every call prefixed with the
// RUSTUP_AUTO_INSTALL value it saw.
function fakeTools({
  targo = true,
  served = [],
  toolchains = 'stable-fake (default)',
  targets = ''
} = {}) {
  const sysroot = join(fixtureDir, 'sysroot')
  for (const triple of served) {
    mkdirSync(join(sysroot, triple, 'lib'), { recursive: true })
    writeFileSync(join(sysroot, triple, 'lib', 'libstd-fake.rlib'), '')
  }
  if (targo) {
    writeTool('targo', [
      "if (!process.argv.includes('--unverified')) process.exit(2)",
      "console.log('targo 0.0.0-fake')"
    ])
  }
  writeTool('trustc', [
    "const triple = process.argv[process.argv.indexOf('--target') + 1]",
    `console.log(require('node:path').join(${JSON.stringify(sysroot)}, triple, 'lib'))`
  ])
  const log = join(fixtureDir, 'rustup.log')
  writeTool('rustup', [
    'const args = process.argv.slice(2)',
    `const line = (process.env.RUSTUP_AUTO_INSTALL ?? 'unset') + ' ' + args.join(' ') + '\\n'`,
    `require('node:fs').appendFileSync(${JSON.stringify(log)}, line)`,
    `if (args[0] === 'toolchain') console.log(${JSON.stringify(toolchains)})`,
    `else if (args[0] === 'target') console.log(${JSON.stringify(targets)})`,
    "else if (args[0] === 'which') console.log('/fake/stock/' + args[1])"
  ])
  return {
    // Only the fakes and node itself on PATH, and no inherited CARGO/RUSTC.
    env: { PATH: `${fixtureDir}${delimiter}${dirname(process.execPath)}` },
    rustupCalls: () =>
      existsSync(log) ? readFileSync(log, 'utf8').split('\n').filter(Boolean) : []
  }
}

describe('rust-toolchain-lane', () => {
  beforeEach(() => {
    fixtureDir = mkdtempSync(join(tmpdir(), 'orca-rust-lane-'))
    vi.spyOn(console, 'log').mockImplementation(() => {})
  })
  afterEach(() => {
    vi.restoreAllMocks()
    rmSync(fixtureDir, { recursive: true, force: true })
  })

  it('maps Node platform/arch to Rust host triples and requires Trust on the sealed host', () => {
    expect(hostTriple('darwin', 'arm64')).toBe('aarch64-apple-darwin')
    expect(hostTriple('darwin', 'x64')).toBe('x86_64-apple-darwin')
    expect(hostTriple('linux', 'x64')).toBe(OTHER)
    expect(TRUST_REQUIRED_TRIPLES.has('aarch64-apple-darwin')).toBe(true)
  })

  itOnPosix('refuses a $CARGO that is upstream cargo posing as targo', () => {
    const tools = fakeTools()
    const upstream = writeTool('upstream-cargo', [
      `console.error("error: unexpected argument '--unverified' found")`,
      'process.exit(1)'
    ])
    const env = { ...tools.env, CARGO: upstream }
    expect(() => resolveTargo(env)).toThrow(RustToolchainError)
    expect(() => resolveTargo(env)).toThrow(/is not targo/)
  })

  itOnPosix('errors on a Trust-required triple without targo instead of using stock', () => {
    const tools = fakeTools({ targo: false, targets: 'aarch64-apple-darwin' })
    const env = { ...tools.env, CARGO: join(fixtureDir, 'no-such-targo') }
    expect(() =>
      cargoInvocation({ verb: 'build', triple: 'aarch64-apple-darwin', env, label: 'test' })
    ).toThrow(/no stock fallback/)
    expect(tools.rustupCalls()).toEqual([])
  })

  itOnPosix('errors on a Trust-required triple whose installed Trust std is missing', () => {
    const tools = fakeTools({ served: [], targets: 'aarch64-apple-darwin' })
    expect(() =>
      cargoInvocation({
        verb: 'build',
        triple: 'aarch64-apple-darwin',
        env: tools.env,
        label: 'test'
      })
    ).toThrow(/ships no std for aarch64-apple-darwin/)
    expect(tools.rustupCalls()).toEqual([])
  })

  itOnPosix('builds any triple the installed Trust sysroot ships with targo --unverified', () => {
    const tools = fakeTools({ served: [OTHER] })
    const invocation = cargoInvocation({
      verb: 'build',
      args: ['--release'],
      triple: OTHER,
      env: tools.env,
      label: 'test',
      stockTargetDir: join(fixtureDir, 'stock')
    })
    expect(invocation.lane).toBe('trust')
    expect(invocation.command).toBe('targo')
    expect(invocation.args).toEqual(['--unverified', 'build', '--release', '--target', OTHER])
    expect(invocation.targetDir).toBeNull()
    expect(tools.rustupCalls()).toEqual([])
  })

  itOnPosix('takes the STOCK EXCEPTION only when the probe finds no Trust std', () => {
    const tools = fakeTools({ served: [], targets: WASM })
    const probe = probeTrustTriple(WASM, tools.env)
    expect(probe.served).toBe(false)
    expect(probe.detail).toMatch(/ships no std for wasm32-unknown-unknown/)
    const stockDir = join(fixtureDir, 'stock')
    const invocation = cargoInvocation({
      verb: 'build',
      triple: WASM,
      env: tools.env,
      label: 'test',
      stockTargetDir: stockDir
    })
    expect(invocation.lane).toBe('stock-exception')
    expect(invocation.command).toBe('/fake/stock/cargo')
    expect(invocation.args).toEqual(['build', '--target', WASM])
    expect(invocation.env.RUSTC).toBe('/fake/stock/rustc')
    expect(invocation.env.CARGO_TARGET_DIR).toBe(stockDir)
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/STOCK EXCEPTION for wasm32/))
  })

  itOnPosix('never asks rustup to resolve (and so install) a missing stock toolchain', () => {
    const tools = fakeTools({ toolchains: 'nightly-fake (default)', targets: WASM })
    expect(() => stockExceptionToolchain({ targets: [WASM], env: tools.env })).toThrow(
      /`stable`, which is not installed/
    )
    expect(tools.rustupCalls().map((call) => call.split(' ')[1])).toEqual(['toolchain'])
  })

  itOnPosix('never asks rustup to resolve (and so install) a missing stock target', () => {
    const tools = fakeTools({ targets: 'aarch64-apple-darwin' })
    expect(() => stockExceptionToolchain({ targets: [WASM], env: tools.env })).toThrow(
      /lacks the std for wasm32-unknown-unknown/
    )
    expect(tools.rustupCalls().map((call) => call.split(' ')[1])).toEqual(['toolchain', 'target'])
  })

  itOnPosix('runs every rustup query with RUSTUP_AUTO_INSTALL=0', () => {
    const tools = fakeTools({ targets: WASM })
    stockExceptionToolchain({ targets: [WASM], env: { ...tools.env, RUSTUP_AUTO_INSTALL: '1' } })
    const calls = tools.rustupCalls()
    expect(calls.map((call) => call.split(' ')[1])).toEqual([
      'toolchain',
      'target',
      'which',
      'which'
    ])
    expect(calls.every((call) => call.startsWith('0 '))).toBe(true)
  })

  itOnPosix('puts --target before a `--` that hands the rest of argv to the program', () => {
    const tools = fakeTools({ served: [OTHER] })
    const invocation = cargoInvocation({
      verb: 'run',
      args: ['-p', 'orca-parity', '--', 'vectors', 'out.json'],
      triple: OTHER,
      env: tools.env,
      label: 'test'
    })
    expect(invocation.args).toEqual([
      '--unverified',
      'run',
      '-p',
      'orca-parity',
      '--target',
      OTHER,
      '--',
      'vectors',
      'out.json'
    ])
  })
})
