import { describe, expect, it, vi } from 'vitest'
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  NODE_PTY_PATCH_PAYLOAD_DIR,
  collectPatchedNodePtyPayload,
  deliverPatchedNodePtyFiles
} from './ssh-relay-node-pty-patch-delivery'
import { getRemoteHostPlatform } from './ssh-remote-platform'

function makeRelayDirWithPayload(): string {
  const relayDir = mkdtempSync(join(tmpdir(), 'relay-npty-'))
  const libDir = join(relayDir, NODE_PTY_PATCH_PAYLOAD_DIR, 'lib')
  mkdirSync(libDir, { recursive: true })
  writeFileSync(join(libDir, 'conpty_console_list_agent.js'), 'patched-agent')
  writeFileSync(join(libDir, 'unixTerminal.js'), 'patched-unix-terminal')
  return relayDir
}

describe('collectPatchedNodePtyPayload', () => {
  it('returns package-relative posix paths with contents', () => {
    const relayDir = makeRelayDirWithPayload()
    try {
      const payload = collectPatchedNodePtyPayload(relayDir)
      expect(payload.map((f) => f.packageRelativePosixPath).sort()).toEqual([
        'lib/conpty_console_list_agent.js',
        'lib/unixTerminal.js'
      ])
      expect(
        payload.find((f) => f.packageRelativePosixPath === 'lib/conpty_console_list_agent.js')
          ?.contents
      ).toBe('patched-agent')
    } finally {
      rmSync(relayDir, { recursive: true, force: true })
    }
  })

  it('returns an empty payload when the dir is absent (older local relay build)', () => {
    const relayDir = mkdtempSync(join(tmpdir(), 'relay-npty-empty-'))
    try {
      expect(collectPatchedNodePtyPayload(relayDir)).toEqual([])
    } finally {
      rmSync(relayDir, { recursive: true, force: true })
    }
  })
})

describe('deliverPatchedNodePtyFiles', () => {
  it('writes each payload file into the remote node-pty install (posix)', async () => {
    const relayDir = makeRelayDirWithPayload()
    try {
      const writes: [string, string][] = []
      const delivered = await deliverPatchedNodePtyFiles({
        localRelayDir: relayDir,
        remoteDir: '/home/dev/.orca-remote/relay-0.1.0+abc',
        hostPlatform: getRemoteHostPlatform('linux-x64'),
        writeRemoteFile: async (remotePath, contents) => {
          writes.push([remotePath, contents])
        }
      })
      expect(delivered).toBe(2)
      expect(writes.map(([p]) => p).sort()).toEqual([
        '/home/dev/.orca-remote/relay-0.1.0+abc/node_modules/node-pty/lib/conpty_console_list_agent.js',
        '/home/dev/.orca-remote/relay-0.1.0+abc/node_modules/node-pty/lib/unixTerminal.js'
      ])
    } finally {
      rmSync(relayDir, { recursive: true, force: true })
    }
  })

  it('targets forward-slash Windows remote paths', async () => {
    const relayDir = makeRelayDirWithPayload()
    try {
      const writes: string[] = []
      await deliverPatchedNodePtyFiles({
        localRelayDir: relayDir,
        remoteDir: 'C:/Users/dev/.orca-remote/relay-0.1.0+abc',
        hostPlatform: getRemoteHostPlatform('win32-x64'),
        writeRemoteFile: async (remotePath) => {
          writes.push(remotePath)
        }
      })
      expect(writes.sort()).toEqual([
        'C:/Users/dev/.orca-remote/relay-0.1.0+abc/node_modules/node-pty/lib/conpty_console_list_agent.js',
        'C:/Users/dev/.orca-remote/relay-0.1.0+abc/node_modules/node-pty/lib/unixTerminal.js'
      ])
    } finally {
      rmSync(relayDir, { recursive: true, force: true })
    }
  })

  it('propagates writer failures so the caller can degrade loudly', async () => {
    const relayDir = makeRelayDirWithPayload()
    try {
      await expect(
        deliverPatchedNodePtyFiles({
          localRelayDir: relayDir,
          remoteDir: '/home/dev/.orca-remote/relay-0.1.0+abc',
          hostPlatform: getRemoteHostPlatform('linux-x64'),
          writeRemoteFile: vi.fn().mockRejectedValue(new Error('sftp write failed'))
        })
      ).rejects.toThrow('sftp write failed')
    } finally {
      rmSync(relayDir, { recursive: true, force: true })
    }
  })
})

describe('local node-pty patch payload source', () => {
  // Why: the delivery mechanism ships whatever the local pnpm patch produced.
  // If the patch loses these fixes, remote hosts silently regress (#9586).
  it('local patched node-pty carries the ConPTY AttachConsole fallback', () => {
    const agent = readFileSync(
      require.resolve('node-pty/lib/conpty_console_list_agent.js'),
      'utf-8'
    )
    expect(agent).toContain('consoleProcessList = [shellPid]')
  })
})
