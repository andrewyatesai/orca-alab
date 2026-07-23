import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import { clearDaemonRosterCache, resolveClaudeForkParentSessionId } from './daemon-roster-resolver'

const PARENT_ID = 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee'
const CHILD_ID = 'bbbbbbbb-cccc-4ddd-8eee-ffffffffffff'

function makeConfigDir(): { configDir: string; transcriptPath: string; rosterDir: string } {
  const configDir = mkdtempSync(join(tmpdir(), 'orca-roster-'))
  const projectDir = join(configDir, 'projects', '-tmp-proj')
  mkdirSync(projectDir, { recursive: true })
  const rosterDir = join(configDir, 'daemon')
  mkdirSync(rosterDir, { recursive: true })
  return { configDir, transcriptPath: join(projectDir, `${CHILD_ID}.jsonl`), rosterDir }
}

function writeRoster(rosterDir: string): void {
  writeFileSync(
    join(rosterDir, 'roster.json'),
    JSON.stringify({
      workers: {
        [CHILD_ID.slice(0, 8)]: {
          sessionId: CHILD_ID,
          dispatch: { launch: { mode: 'resume', sessionId: `/x/${PARENT_ID}.jsonl` } }
        }
      }
    })
  )
}

const dirs: string[] = []

beforeEach(() => {
  clearDaemonRosterCache()
})

afterEach(() => {
  clearDaemonRosterCache()
  while (dirs.length > 0) {
    const dir = dirs.pop()
    if (dir) {
      rmSync(dir, { recursive: true, force: true })
    }
  }
})

describe('resolveClaudeForkParentSessionId', () => {
  it('resolves the fork parent from the daemon roster', () => {
    const { configDir, transcriptPath, rosterDir } = makeConfigDir()
    dirs.push(configDir)
    writeRoster(rosterDir)
    expect(resolveClaudeForkParentSessionId(CHILD_ID, transcriptPath, 1000)).toBe(PARENT_ID)
  })

  it('rejects a relative transcript path (cannot locate a real config dir)', () => {
    expect(
      resolveClaudeForkParentSessionId(CHILD_ID, `projects/-tmp-proj/${CHILD_ID}.jsonl`, 1000)
    ).toBeNull()
  })

  it('fails closed on a symlinked roster.json instead of following it', () => {
    const { configDir, transcriptPath, rosterDir } = makeConfigDir()
    dirs.push(configDir)
    // Point roster.json at a real, valid roster elsewhere; a symlink must still
    // be refused so a hook payload cannot redirect the read.
    const target = mkdtempSync(join(tmpdir(), 'orca-roster-target-'))
    dirs.push(target)
    writeFileSync(
      join(target, 'roster.json'),
      JSON.stringify({
        workers: {
          [CHILD_ID.slice(0, 8)]: {
            sessionId: CHILD_ID,
            dispatch: { launch: { sessionId: `/x/${PARENT_ID}.jsonl` } }
          }
        }
      })
    )
    symlinkSync(join(target, 'roster.json'), join(rosterDir, 'roster.json'))
    expect(resolveClaudeForkParentSessionId(CHILD_ID, transcriptPath, 1000)).toBeNull()
  })

  it('fails closed on an oversized roster.json instead of reading it', () => {
    const { configDir, transcriptPath, rosterDir } = makeConfigDir()
    dirs.push(configDir)
    // A > 512 KiB file must be skipped BEFORE it is read/parsed, even though it
    // holds a valid parent mapping: the bounded read must not resolve it, so a
    // crafted/corrupt file cannot freeze the main thread or OOM.
    const roster = {
      workers: {
        [CHILD_ID.slice(0, 8)]: {
          sessionId: CHILD_ID,
          dispatch: { launch: { sessionId: `/x/${PARENT_ID}.jsonl` } }
        }
      },
      pad: 'x'.repeat(600 * 1024)
    }
    writeFileSync(join(rosterDir, 'roster.json'), JSON.stringify(roster))
    expect(resolveClaudeForkParentSessionId(CHILD_ID, transcriptPath, 1000)).toBeNull()
  })

  it('memoizes the parsed roster within the TTL and re-reads after it expires', () => {
    const { configDir, transcriptPath, rosterDir } = makeConfigDir()
    dirs.push(configDir)
    writeRoster(rosterDir)
    // First resolve reads and caches.
    expect(resolveClaudeForkParentSessionId(CHILD_ID, transcriptPath, 1000)).toBe(PARENT_ID)
    // Delete the file — a second resolve inside the TTL must still hit the cache,
    // proving the hot hook path does not re-read disk on every event.
    rmSync(join(rosterDir, 'roster.json'))
    expect(resolveClaudeForkParentSessionId(CHILD_ID, transcriptPath, 1500)).toBe(PARENT_ID)
    // Past the TTL it re-reads and, finding the file gone, fails open to null.
    expect(resolveClaudeForkParentSessionId(CHILD_ID, transcriptPath, 5000)).toBeNull()
  })
})
