// Fed §2.4, SSH routing half: for SSH-provider panes the search authority is
// the LOCAL runtime's per-PTY emulator — the same one fed by the SSH relay
// session's onPtyData path (ssh-relay-session.ts) — so `terminal.search`
// works for ssh: panes without any relay-side scrollback. This drives the
// REAL emulator (napi addon) through the runtime's ingestion path with an
// ssh connectionId, exactly as relay bytes arrive.
import { describe, expect, it, vi } from 'vitest'
import { OrcaRuntimeService } from './orca-runtime'

vi.mock('electron', () => ({
  BrowserWindow: { fromId: vi.fn(() => null) },
  webContents: { fromId: vi.fn(() => null) },
  ipcMain: {
    on: vi.fn(),
    removeListener: vi.fn()
  },
  app: { getPath: vi.fn(() => '/tmp') }
}))

const TEST_WINDOW_ID = 1

function makeRuntimeWithSshPane(): { runtime: OrcaRuntimeService; ptyId: string } {
  const runtime = new OrcaRuntimeService()
  const ptyId = 'pty-ssh-1'
  runtime.syncWindowGraph(TEST_WINDOW_ID, {
    tabs: [
      {
        tabId: 'tab-1',
        worktreeId: 'repo-1::/tmp/worktree-a',
        title: 'ssh terminal',
        activeLeafId: 'pane:1',
        layout: null
      }
    ],
    leaves: [
      {
        tabId: 'tab-1',
        worktreeId: 'repo-1::/tmp/worktree-a',
        leafId: 'pane:1',
        paneRuntimeId: 1,
        ptyId
      }
    ]
  })
  // The relay session registers SSH ptys with their connection id, then feeds
  // bytes through the same onPtyData entry local ptys use.
  runtime.registerPty(ptyId, 'repo-1::/tmp/worktree-a', 'ssh-1', {
    tabId: 'tab-1',
    leafId: 'pane:1'
  })
  return { runtime, ptyId }
}

describe('terminal search over an SSH-provider pane', () => {
  it('searches relay-fed bytes through the runtime emulator with stable rows', async () => {
    const { runtime, ptyId } = makeRuntimeWithSshPane()
    runtime.onPtyData(ptyId, 'remote build starting\r\n', 1)
    runtime.onPtyData(ptyId, '\x1b[1;31merror:\x1b[0m ENOSPC on host\r\n', 2)
    runtime.onPtyData(ptyId, 'done\r\n', 3)

    const handle = (await runtime.listTerminals()).terminals[0]?.handle
    expect(handle).toBeTruthy()

    const outcome = await runtime.searchTerminalScrollback(handle!, { query: 'enospc' })
    expect(outcome.available).toBe(true)
    expect(outcome.total).toBe(1)
    // ANSI is stripped by the emulator parse — the snippet is plain text.
    expect(outcome.matches[0].line).toBe('error: ENOSPC on host')
    expect(outcome.hostCols).toBeGreaterThan(0)
    expect(outcome.incarnation).not.toBeNull()

    const context = await runtime.terminalSearchContext(handle!, {
      hostRow: outcome.matches[0].hostRow,
      before: 1,
      after: 1
    })
    expect(context.available).toBe(true)
    expect(context.lines).toContain('error: ENOSPC on host')
    expect(context.lines).toContain('remote build starting')
  })

  it('reports unavailable (never throws) for a pane without headless state', async () => {
    const { runtime } = makeRuntimeWithSshPane()
    // No bytes ingested → no emulator exists yet for this pty.
    const handle = (await runtime.listTerminals()).terminals[0]?.handle
    expect(handle).toBeTruthy()
    const outcome = await runtime.searchTerminalScrollback(handle!, { query: 'anything' })
    expect(outcome).toMatchObject({ available: false, matches: [], total: 0 })
  })
})
