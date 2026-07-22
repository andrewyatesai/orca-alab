import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest'
import { mkdtempSync, rmSync, statSync, utimesSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  makeTerminalScrollbackSnapshotRef,
  writeTerminalScrollbackSnapshotSync
} from './terminal-scrollback-snapshots'
import {
  readTerminalScrollbackSnapshotOlderChunkSync,
  readTerminalScrollbackSnapshotTailSync
} from './terminal-scrollback-snapshot-deep-read'
import {
  TERMINAL_SCROLLBACK_OLDER_CHUNK_BYTE_LIMIT,
  TERMINAL_SCROLLBACK_REPLAY_BYTE_LIMIT,
  TERMINAL_SCROLLBACK_STORE_BYTE_LIMIT
} from '../shared/terminal-scrollback-limits'

vi.mock('electron', () => ({
  app: { getPath: vi.fn(() => join(tmpdir(), 'orca-deep-restore-unused-legacy-root')) }
}))

// Multibyte-heavy line so 512KB chunk boundaries land inside UTF-8 sequences.
function buildSnapshotBuffer(totalApproxBytes: number): string {
  const line = `log \u{1F980} café 你好 \x1b[31mred\x1b[0m ${'x'.repeat(40)}\r\n`
  const lineBytes = Buffer.byteLength(line, 'utf-8')
  return line.repeat(Math.ceil(totalApproxBytes / lineBytes))
}

describe('terminal scrollback deep-restore reads (P5)', () => {
  let dir: string
  let storage: { snapshotRoot: string }

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'orca-scrollback-deep-'))
    storage = { snapshotRoot: dir }
  })

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true })
  })

  function writeSnapshot(buffer: string): string {
    const ref = writeTerminalScrollbackSnapshotSync({
      tabId: 'tab-1',
      leafId: 'leaf-1',
      buffer,
      storage
    })
    expect(ref).toBe(makeTerminalScrollbackSnapshotRef('tab-1', 'leaf-1'))
    return ref!
  }

  function readAllOlderChunks(
    ref: string,
    tail: NonNullable<ReturnType<typeof readTerminalScrollbackSnapshotTailSync>>
  ): { chunks: string[]; text: string } {
    const chunks: string[] = []
    let cursor = tail.olderChunkCursor
    while (cursor < tail.olderEndOffset) {
      const chunk = readTerminalScrollbackSnapshotOlderChunkSync(
        { ref, cursor, endOffset: tail.olderEndOffset, fingerprint: tail.fingerprint },
        storage
      )
      expect(chunk).not.toBeNull()
      expect(chunk!.nextCursor).toBeGreaterThan(cursor)
      chunks.push(chunk!.text)
      cursor = chunk!.nextCursor
    }
    return { chunks, text: chunks.join('') }
  }

  it('tail + streamed older chunks reassemble the full 5MB store losslessly', () => {
    const buffer = buildSnapshotBuffer(TERMINAL_SCROLLBACK_STORE_BYTE_LIMIT + 64 * 1024)
    const ref = writeSnapshot(buffer)

    const tail = readTerminalScrollbackSnapshotTailSync(ref, storage)
    expect(tail).not.toBeNull()
    // The sync (renderer-blocking) portion stays bounded exactly as before P5.
    expect(Buffer.byteLength(tail!.text, 'utf-8')).toBeLessThanOrEqual(
      TERMINAL_SCROLLBACK_REPLAY_BYTE_LIMIT
    )

    const older = readAllOlderChunks(ref, tail!)
    // Every streamed chunk stays within the per-IPC byte budget.
    for (const chunk of older.chunks) {
      expect(Buffer.byteLength(chunk, 'utf-8')).toBeLessThanOrEqual(
        TERMINAL_SCROLLBACK_OLDER_CHUNK_BYTE_LIMIT
      )
    }
    // No replacement characters: chunk boundaries never split a codepoint.
    expect(older.text).not.toContain('�')
    expect(tail!.text).not.toContain('�')

    const reassembled = older.text + tail!.text
    // The store writer keeps the trailing 5MB (UTF-8 aligned); the reassembly must match it byte-for-byte.
    const storedBytes = statSync(join(dir, `${ref}.bin`)).size
    expect(Buffer.byteLength(reassembled, 'utf-8')).toBe(storedBytes)
    expect(buffer.endsWith(reassembled)).toBe(true)
  })

  it('reports no older region for snapshots within the sync tail limit', () => {
    const ref = writeSnapshot('short scrollback\r\n')
    const tail = readTerminalScrollbackSnapshotTailSync(ref, storage)
    expect(tail).not.toBeNull()
    expect(tail!.text).toBe('short scrollback\r\n')
    expect(tail!.olderChunkCursor).toBe(tail!.olderEndOffset)
  })

  it('aborts older-chunk reads when the snapshot was rewritten (fingerprint mismatch)', () => {
    const buffer = buildSnapshotBuffer(2 * 1024 * 1024)
    const ref = writeSnapshot(buffer)
    const tail = readTerminalScrollbackSnapshotTailSync(ref, storage)!
    expect(tail.olderChunkCursor).toBeLessThan(tail.olderEndOffset)

    // Rewrite with different content AND a different mtime — an atomic-rename
    // replace between the tail read and the chunk reads.
    writeFileSync(join(dir, `${ref}.bin`), buildSnapshotBuffer(1024 * 1024))
    utimesSync(join(dir, `${ref}.bin`), new Date(), new Date(Date.now() + 5_000))

    expect(
      readTerminalScrollbackSnapshotOlderChunkSync(
        {
          ref,
          cursor: tail.olderChunkCursor,
          endOffset: tail.olderEndOffset,
          fingerprint: tail.fingerprint
        },
        storage
      )
    ).toBeNull()
  })

  it('rejects out-of-range and non-integer chunk cursors', () => {
    const ref = writeSnapshot(buildSnapshotBuffer(1024 * 1024))
    const tail = readTerminalScrollbackSnapshotTailSync(ref, storage)!
    const base = { ref, endOffset: tail.olderEndOffset, fingerprint: tail.fingerprint }
    expect(
      readTerminalScrollbackSnapshotOlderChunkSync({ ...base, cursor: -1 }, storage)
    ).toBeNull()
    expect(
      readTerminalScrollbackSnapshotOlderChunkSync({ ...base, cursor: tail.olderEndOffset }, storage)
    ).toBeNull()
    expect(
      readTerminalScrollbackSnapshotOlderChunkSync({ ...base, cursor: 0.5 }, storage)
    ).toBeNull()
    expect(
      readTerminalScrollbackSnapshotOlderChunkSync(
        { ref, cursor: 0, endOffset: Number.MAX_SAFE_INTEGER + 1, fingerprint: tail.fingerprint },
        storage
      )
    ).toBeNull()
  })

  it('caps a buffer retained in session JSON when the snapshot write fails', async () => {
    const { migrateWorkspaceSessionTerminalScrollbackSnapshots } = await import(
      './terminal-scrollback-snapshots'
    )
    const oversized = 'y'.repeat(1024 * 1024)
    const session = {
      terminalLayoutsByTabId: {
        'tab-1': {
          root: null,
          activeLeafId: null,
          expandedLeafId: null,
          buffersByLeafId: { 'leaf-1': oversized }
        }
      }
    } as never
    // Unwritable root forces the ref write to fail, exercising the JSON fallback.
    const migrated = migrateWorkspaceSessionTerminalScrollbackSnapshots(session, {
      snapshotRoot: join(dir, 'missing\0invalid')
    })
    const retained = (
      migrated.session as {
        terminalLayoutsByTabId: Record<string, { buffersByLeafId?: Record<string, string> }>
      }
    ).terminalLayoutsByTabId['tab-1'].buffersByLeafId?.['leaf-1']
    expect(retained).toBeDefined()
    // Why 512KB: failed migrations fall back to session JSON, which keeps the small session bound.
    expect(Buffer.byteLength(retained!, 'utf-8')).toBeLessThanOrEqual(512 * 1024)
  })
})
