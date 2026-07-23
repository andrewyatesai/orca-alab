import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { WorkspacePort, WorkspacePortScanResult } from '../../../shared/workspace-ports'
import type * as RuntimeRpcClientModule from '@/runtime/runtime-rpc-client'

const callRuntimeRpc = vi.fn()

vi.mock('@/runtime/runtime-rpc-client', async (importOriginal) => {
  const actual = await importOriginal<typeof RuntimeRpcClientModule>()
  return { ...actual, callRuntimeRpc }
})

const { runWorkspacePortScanForTarget } = await import('./workspace-port-scan-client')
const { RuntimeRpcCallError } = await import('@/runtime/runtime-rpc-client')

function validPort(): WorkspacePort {
  return {
    id: 'tcp:3000',
    bindHost: '0.0.0.0',
    connectHost: '127.0.0.1',
    port: 3000,
    pid: 42,
    processName: 'node',
    protocol: 'http',
    kind: 'workspace',
    owner: {
      worktreeId: 'wt-1',
      repoId: 'repo-1',
      displayName: 'main',
      path: '/repo',
      confidence: 'cwd'
    }
  }
}

function validScan(overrides: Partial<WorkspacePortScanResult> = {}): WorkspacePortScanResult {
  return {
    platform: 'darwin',
    scannedAt: 123,
    ports: [validPort()],
    ...overrides
  }
}

const scanMock = vi.fn()

describe('runWorkspacePortScanForTarget payload validation (#9776)', () => {
  beforeEach(() => {
    scanMock.mockReset()
    callRuntimeRpc.mockReset()
    vi.stubGlobal('window', { api: { workspacePorts: { scan: scanMock } } })
  })
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('accepts a well-formed local scan payload', async () => {
    scanMock.mockResolvedValue(validScan())
    await expect(runWorkspacePortScanForTarget({ kind: 'local' })).resolves.toEqual(validScan())
  })

  it('rejects a local payload whose ports array holds a malformed row', async () => {
    scanMock.mockResolvedValue(
      validScan({ ports: [{ id: 'tcp:3000' } as unknown as WorkspacePort] })
    )
    await expect(runWorkspacePortScanForTarget({ kind: 'local' })).rejects.toThrow(
      'Workspace port scan returned an invalid response.'
    )
  })

  it('rejects a workspace port whose owner is missing required fields', async () => {
    const port = { ...validPort(), owner: { worktreeId: 'wt-1' } } as unknown as WorkspacePort
    scanMock.mockResolvedValue(validScan({ ports: [port] }))
    await expect(runWorkspacePortScanForTarget({ kind: 'local' })).rejects.toThrow(
      'Workspace port scan returned an invalid response.'
    )
  })

  it('rejects an unknown protocol injected by a hostile runtime', async () => {
    const port = { ...validPort(), protocol: 'ftp' } as unknown as WorkspacePort
    callRuntimeRpc.mockResolvedValue(validScan({ ports: [port] }))
    await expect(
      runWorkspacePortScanForTarget({ kind: 'environment', environmentId: 'env-1' })
    ).rejects.toThrow('Workspace port scan returned an invalid response.')
  })

  it('validates a well-formed remote runtime payload', async () => {
    callRuntimeRpc.mockResolvedValue(validScan())
    await expect(
      runWorkspacePortScanForTarget({ kind: 'environment', environmentId: 'env-1' })
    ).resolves.toEqual(validScan())
  })

  it('returns an unavailable placeholder when the runtime lacks the method', async () => {
    callRuntimeRpc.mockRejectedValue(
      new RuntimeRpcCallError({
        ok: false,
        error: { code: 'method_not_found', message: 'nope' }
      } as never)
    )
    const result = await runWorkspacePortScanForTarget({
      kind: 'environment',
      environmentId: 'env-1'
    })
    expect(result.ports).toEqual([])
    expect(result.unavailableReason).toContain('does not support workspace port management')
  })
})
