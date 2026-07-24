import { describe, expect, it, vi } from 'vitest'
import { RpcDispatcher } from '../dispatcher'
import type { RpcRequest } from '../core'
import type { OrcaRuntimeService } from '../../orca-runtime'
import { FILE_METHODS } from './files'

function makeRequest(method: string, params?: unknown): RpcRequest {
  return { id: 'req-1', authToken: 'tok', method, params }
}

describe('files.writeBase64Chunk 4-alignment guard', () => {
  // Why: each chunk is decoded independently and appended, so a non-4-aligned
  // chunk silently corrupts the file while still returning ok:true. 'TWFuIGlz'
  // is valid base64 (charset ok) len 8 %4===0; 'TWFuIG' (len 6, %4===2) and
  // 'TWFuIGl' (len 7, %4===3) are charset-valid but misaligned and must reject.
  it.each([
    ['length %4===2', 'TWFuIG'],
    ['length %4===3', 'TWFuIGl']
  ])(
    'rejects a misaligned base64 chunk (%s) before appending corrupt bytes',
    async (_name, contentBase64) => {
      const runtime = {
        getRuntimeId: () => 'test-runtime',
        writeFileExplorerFileBase64Chunk: vi.fn().mockResolvedValue({ ok: true })
      } as unknown as OrcaRuntimeService
      const dispatcher = new RpcDispatcher({ runtime, methods: FILE_METHODS })

      const response = await dispatcher.dispatch(
        makeRequest('files.writeBase64Chunk', {
          worktree: 'id:wt-1',
          relativePath: 'assets/video.mov',
          contentBase64,
          append: true
        })
      )

      expect(response).toMatchObject({ ok: false, error: { code: 'invalid_argument' } })
      expect(runtime.writeFileExplorerFileBase64Chunk).not.toHaveBeenCalled()
    }
  )

  it.each([
    ['4-aligned chunk', 'TWFuIGlz'],
    ['explicit empty chunk', '']
  ])('accepts a lossless base64 chunk (%s)', async (_name, contentBase64) => {
    const runtime = {
      getRuntimeId: () => 'test-runtime',
      writeFileExplorerFileBase64Chunk: vi.fn().mockResolvedValue({ ok: true })
    } as unknown as OrcaRuntimeService
    const dispatcher = new RpcDispatcher({ runtime, methods: FILE_METHODS })

    const response = await dispatcher.dispatch(
      makeRequest('files.writeBase64Chunk', {
        worktree: 'id:wt-1',
        relativePath: 'assets/video.mov',
        contentBase64,
        append: true
      })
    )

    expect(response).toMatchObject({ ok: true })
    expect(runtime.writeFileExplorerFileBase64Chunk).toHaveBeenCalledWith(
      'id:wt-1',
      'assets/video.mov',
      contentBase64,
      true
    )
  })
})

describe('files.unwatch connection authorization', () => {
  it('refuses to tear down a subscription owned by another connection', async () => {
    const cleanupSubscriptionAndWait = vi.fn().mockResolvedValue(undefined)
    const runtime = {
      getRuntimeId: () => 'test-runtime',
      cleanupSubscriptionAndWait
    } as unknown as OrcaRuntimeService
    const dispatcher = new RpcDispatcher({ runtime, methods: FILE_METHODS })

    // Connection B tries to tear down connection A's watch stream by id.
    const foreign: string[] = []
    await dispatcher.dispatchStreaming(
      makeRequest('files.unwatch', { subscriptionId: 'files-watch-connA-3' }),
      (message) => foreign.push(message),
      { connectionId: 'connB' }
    )
    expect(JSON.parse(foreign[0]!)).toMatchObject({ ok: true, result: { unsubscribed: false } })
    expect(cleanupSubscriptionAndWait).not.toHaveBeenCalled()

    // The owning connection can still tear its own subscription down.
    const own: string[] = []
    await dispatcher.dispatchStreaming(
      makeRequest('files.unwatch', { subscriptionId: 'files-watch-connA-3' }),
      (message) => own.push(message),
      { connectionId: 'connA' }
    )
    expect(JSON.parse(own[0]!)).toMatchObject({ ok: true, result: { unsubscribed: true } })
    expect(cleanupSubscriptionAndWait).toHaveBeenCalledWith('files-watch-connA-3')
  })
})
