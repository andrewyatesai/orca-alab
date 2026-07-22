import { mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import { join } from 'node:path'
import { afterAll, describe, expect, it } from 'vitest'
import { defaultCorpusPath, ensureFloodCorpus, floodCorpusLine } from './daemon-flood-corpus.mjs'

const scratch = mkdtempSync(join(os.tmpdir(), 'daemon-flood-corpus-test-'))
afterAll(() => rmSync(scratch, { recursive: true, force: true }))

describe('floodCorpusLine', () => {
  it('matches stream_flood_bench.rs generate_corpus byte-for-byte', () => {
    // Rust: format!("\x1b[3{}mINFO\x1b[0m step {:010} lorem ipsum ...\n", i % 8, i)
    expect(floodCorpusLine(0)).toBe(
      '\x1b[30mINFO\x1b[0m step 0000000000 lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod\n'
    )
    expect(floodCorpusLine(12345)).toBe(
      '\x1b[31mINFO\x1b[0m step 0000012345 lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod\n'
    )
  })

  it('cycles the SGR color through 8 values', () => {
    expect(floodCorpusLine(7).startsWith('\x1b[37m')).toBe(true)
    expect(floodCorpusLine(8).startsWith('\x1b[30m')).toBe(true)
  })
})

describe('ensureFloodCorpus', () => {
  it('writes at least the requested MB of flood lines', async () => {
    const path = join(scratch, 'one-mb.vt')
    const size = await ensureFloodCorpus(path, 1)
    expect(size).toBeGreaterThanOrEqual(1_000_000)
    expect(size).toBe(statSync(path).size)
    const head = readFileSync(path).subarray(0, floodCorpusLine(0).length).toString()
    expect(head).toBe(floodCorpusLine(0))
  })

  it('reuses an existing corpus untouched instead of regenerating', async () => {
    const path = join(scratch, 'preexisting.vt')
    writeFileSync(path, 'tiny')
    expect(await ensureFloodCorpus(path, 1)).toBe(4)
    expect(readFileSync(path, 'utf8')).toBe('tiny')
  })
})

describe('defaultCorpusPath', () => {
  it('is size-keyed so different --mb runs never collide', () => {
    expect(defaultCorpusPath(200)).not.toBe(defaultCorpusPath(500))
    expect(defaultCorpusPath(500).endsWith('orca-daemon-flood-500mb.vt')).toBe(true)
  })
})
