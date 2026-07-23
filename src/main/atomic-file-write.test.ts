import { mkdtempSync, readFileSync, readdirSync, rmSync } from 'node:fs'
import type * as Fs from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Why mock fsyncSync: the durability guarantee (fsync before rename, fsync the dir after) is the whole point of
// this module. Wrap the real fsyncSync so we can assert it actually ran while still exercising real filesystem I/O.
const fsSpies = vi.hoisted(() => ({ fsyncSync: vi.fn() }))
vi.mock('node:fs', async () => {
  const actual = await vi.importActual<typeof Fs>('node:fs')
  return {
    ...actual,
    fsyncSync: (fd: number) => {
      fsSpies.fsyncSync(fd)
      return actual.fsyncSync(fd)
    }
  }
})

import { writeFileAtomicSync, writeFileAtomicAsync } from './atomic-file-write'

describe('atomic-file-write', () => {
  let dir: string

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'orca-atomic-write-'))
    fsSpies.fsyncSync.mockClear()
  })

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true })
  })

  function tmpArtifacts(): string[] {
    return readdirSync(dir).filter((name) => name.includes('.tmp'))
  }

  describe('writeFileAtomicSync', () => {
    it('round-trips content and leaves no tmp file behind', () => {
      const target = join(dir, 'state.json')
      writeFileAtomicSync(target, '{"a":1}')
      expect(readFileSync(target, 'utf-8')).toBe('{"a":1}')
      expect(tmpArtifacts()).toHaveLength(0)
    })

    it('fsyncs the file before returning (power-loss durability)', () => {
      writeFileAtomicSync(join(dir, 'state.json'), 'durable')
      // At least the tmp file fsync must have run (dir fsync too on POSIX).
      expect(fsSpies.fsyncSync).toHaveBeenCalled()
    })

    it('writes a multi-megabyte payload completely (guards against short writeSync truncation)', () => {
      const big = 'x'.repeat(5 * 1024 * 1024)
      const target = join(dir, 'big.json')
      writeFileAtomicSync(target, big)
      expect(readFileSync(target, 'utf-8')).toBe(big)
      expect(tmpArtifacts()).toHaveLength(0)
    })

    it('writes Uint8Array payloads', () => {
      const target = join(dir, 'bytes.bin')
      writeFileAtomicSync(target, new Uint8Array([1, 2, 3, 4]))
      expect(Array.from(readFileSync(target))).toEqual([1, 2, 3, 4])
    })

    it('honors an explicit tmpPath', () => {
      const target = join(dir, 'fixed.json')
      writeFileAtomicSync(target, 'ok', { tmpPath: `${target}.tmp` })
      expect(readFileSync(target, 'utf-8')).toBe('ok')
      expect(tmpArtifacts()).toHaveLength(0)
    })
  })

  describe('writeFileAtomicAsync', () => {
    it('round-trips content and leaves no tmp file behind', async () => {
      const target = join(dir, 'state.json')
      await writeFileAtomicAsync(target, '{"b":2}')
      expect(readFileSync(target, 'utf-8')).toBe('{"b":2}')
      expect(tmpArtifacts()).toHaveLength(0)
    })

    it('writes a multi-megabyte payload completely', async () => {
      const big = 'y'.repeat(3 * 1024 * 1024)
      const target = join(dir, 'big-async.json')
      await writeFileAtomicAsync(target, big)
      expect(readFileSync(target, 'utf-8')).toBe(big)
    })
  })
})
