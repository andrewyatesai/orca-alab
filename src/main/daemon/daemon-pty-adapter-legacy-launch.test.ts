// Regression coverage for the fork-protocol gate on the pre-computed POSIX
// shell launch: a LEGACY adapter (live public Node daemon attached via
// PREVIOUS_DAEMON_PROTOCOL_VERSIONS, e.g. the sleep/wake respawn path) must
// receive the caller's plain shellOverride/env — never the fork-only
// shellArgs/login(1) wrap, which a v18 daemon would misinterpret as the shell
// itself and spawn an interactive `login:` password prompt.
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createServer, type Server, type Socket } from 'node:net'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { DaemonPtyAdapter } from './daemon-pty-adapter'
import { getDaemonSocketPath } from './daemon-spawner'
import { encodeNdjson } from './ndjson'

type RecordedCreateOrAttach = Record<string, unknown>

type FakeDaemon = {
  helloVersions: number[]
  createOrAttachPayloads: RecordedCreateOrAttach[]
  close: () => Promise<void>
}

// A minimal NDJSON daemon that accepts ANY hello version (the real
// DaemonServer only speaks the fork version, so it can't stand in for the
// public v18 daemon) and records createOrAttach payloads verbatim.
function startVersionAgnosticFakeDaemon(socketPath: string): Promise<FakeDaemon> {
  const helloVersions: number[] = []
  const createOrAttachPayloads: RecordedCreateOrAttach[] = []
  const openSockets = new Set<Socket>()
  const server: Server = createServer((socket) => {
    openSockets.add(socket)
    socket.once('close', () => openSockets.delete(socket))
    let buffer = ''
    socket.on('data', (chunk) => {
      buffer += chunk.toString('utf8')
      for (let idx = buffer.indexOf('\n'); idx !== -1; idx = buffer.indexOf('\n')) {
        const line = buffer.slice(0, idx)
        buffer = buffer.slice(idx + 1)
        if (!line.trim()) {
          continue
        }
        const msg = JSON.parse(line) as {
          id?: string
          type?: string
          version?: number
          payload?: RecordedCreateOrAttach
        }
        if (msg.type === 'hello') {
          helloVersions.push(msg.version ?? -1)
          socket.write(encodeNdjson({ type: 'hello', ok: true }))
        } else if (msg.type === 'createOrAttach') {
          createOrAttachPayloads.push(msg.payload ?? {})
          socket.write(
            encodeNdjson({
              id: msg.id,
              ok: true,
              payload: { isNew: true, snapshot: null, pid: 4242, shellState: 'unsupported' }
            })
          )
        }
      }
    })
  })
  return new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(socketPath, () => {
      resolve({
        helloVersions,
        createOrAttachPayloads,
        close: () =>
          new Promise<void>((res) => {
            // Destroy lingering sockets so close() can't hang the suite on a
            // connection the adapter side hasn't torn down yet.
            for (const socket of openSockets) {
              socket.destroy()
            }
            server.close(() => res())
          })
      })
    })
  })
}

describe('DaemonPtyAdapter launch-config protocol gate', () => {
  let dir: string
  let socketPath: string
  let tokenPath: string
  let fakeDaemon: FakeDaemon
  let adapter: DaemonPtyAdapter | null
  const savedDisable = process.env.ORCA_DISABLE_MACOS_LOGIN_SHELL

  beforeEach(async () => {
    dir = mkdtempSync(join(tmpdir(), 'daemon-legacy-launch-'))
    socketPath = getDaemonSocketPath(dir)
    tokenPath = join(dir, 'test.token')
    writeFileSync(tokenPath, 'test-token')
    fakeDaemon = await startVersionAgnosticFakeDaemon(socketPath)
    adapter = null
  })

  afterEach(async () => {
    adapter?.dispose()
    await fakeDaemon.close()
    rmSync(dir, { recursive: true, force: true })
    if (savedDisable === undefined) {
      delete process.env.ORCA_DISABLE_MACOS_LOGIN_SHELL
    } else {
      process.env.ORCA_DISABLE_MACOS_LOGIN_SHELL = savedDisable
    }
  })

  it('legacy adapters pass shellOverride/env through untouched (no shellArgs)', async () => {
    adapter = new DaemonPtyAdapter({ socketPath, tokenPath, protocolVersion: 18 })
    await adapter.spawn({
      cols: 80,
      rows: 24,
      shellOverride: '/bin/zsh',
      env: { FOO: 'bar', SHELL: '/bin/zsh' }
    })

    expect(fakeDaemon.helloVersions).toContain(18)
    expect(fakeDaemon.createOrAttachPayloads).toHaveLength(1)
    const payload = fakeDaemon.createOrAttachPayloads[0]
    // The public Node daemon computes its own launch args from shellOverride;
    // /usr/bin/login here would become an interactive password prompt.
    expect(payload.shellOverride).toBe('/bin/zsh')
    expect(payload.shellArgs).toBeUndefined()
    expect(payload.env).toEqual({ FOO: 'bar', SHELL: '/bin/zsh' })
  })

  it('legacy adapters forward an absent shellOverride as absent', async () => {
    adapter = new DaemonPtyAdapter({ socketPath, tokenPath, protocolVersion: 18 })
    await adapter.spawn({ cols: 80, rows: 24 })

    const payload = fakeDaemon.createOrAttachPayloads[0]
    expect(payload.shellOverride).toBeUndefined()
    expect(payload.shellArgs).toBeUndefined()
  })

  const itOnPosix = process.platform === 'win32' ? it.skip : it
  itOnPosix('fork-protocol adapters ship the pre-computed launch config', async () => {
    // Keep the program deterministic across dev machines; the login(1) wrap
    // has dedicated coverage in daemon-shell-launch-config.test.ts.
    process.env.ORCA_DISABLE_MACOS_LOGIN_SHELL = '1'
    adapter = new DaemonPtyAdapter({ socketPath, tokenPath })
    await adapter.spawn({
      cols: 80,
      rows: 24,
      shellOverride: '/bin/zsh',
      env: { FOO: 'bar' }
    })

    const payload = fakeDaemon.createOrAttachPayloads[0]
    expect(payload.shellOverride).toBe('/bin/zsh')
    expect(payload.shellArgs).toEqual(['-l'])
    expect(payload.env).toMatchObject({
      FOO: 'bar',
      POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD: 'true'
    })
  })
})
