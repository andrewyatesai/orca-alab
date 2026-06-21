import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { createHeadlessEmulator } from './headless-emulator-factory'

// Proves the xterm -> aterm swap is wired as the DEFAULT: createHeadlessEmulator
// returns the aterm-backed engine with no ORCA_RUST_TERMINAL flag set, and there
// is no TypeScript/xterm fallback (a missing addon throws). The addon is built
// by the `build:terminal-addon` test prerequisite.
describe('createHeadlessEmulator (aterm default)', () => {
  let markerDir: string
  let markerPath: string
  const savedFlag = process.env.ORCA_RUST_TERMINAL

  beforeEach(() => {
    markerDir = mkdtempSync(join(tmpdir(), 'orca-engine-'))
    markerPath = join(markerDir, 'engine')
    process.env.ORCA_ENGINE_MARKER = markerPath
    // The flag is gone — aterm is unconditional. Prove it by clearing the flag.
    delete process.env.ORCA_RUST_TERMINAL
  })

  afterEach(() => {
    delete process.env.ORCA_ENGINE_MARKER
    if (savedFlag === undefined) {
      delete process.env.ORCA_RUST_TERMINAL
    } else {
      process.env.ORCA_RUST_TERMINAL = savedFlag
    }
    rmSync(markerDir, { recursive: true, force: true })
  })

  it('selects the aterm engine without any flag and renders output', () => {
    const emulator = createHeadlessEmulator({ cols: 80, rows: 24 })
    emulator.write('\x1b[1;32mhello\x1b[0m world')

    const snapshot = emulator.getSnapshot()
    expect(snapshot.snapshotAnsi).toContain('hello')
    expect(snapshot.cols).toBe(80)
    expect(snapshot.rows).toBe(24)

    // The engine marker proves the native aterm engine went live (no fallback).
    expect(readFileSync(markerPath, 'utf8')).toContain('rust:aterm')
    emulator.dispose()
  })

  it('exposes the full TerminalEmulator interface the daemon session depends on', () => {
    const emulator = createHeadlessEmulator({ cols: 80, rows: 24 }) as unknown as Record<
      string,
      unknown
    >
    for (const method of ['write', 'resize', 'getSnapshot', 'getCwd', 'clearScrollback', 'dispose']) {
      expect(typeof emulator[method]).toBe('function')
    }
  })
})
