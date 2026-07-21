import { beforeEach, describe, expect, it, vi } from 'vitest'

const { ghExecFileAsyncMock } = vi.hoisted(() => ({
  ghExecFileAsyncMock: vi.fn()
}))

vi.mock('../gh-utils', () => ({
  acquire: vi.fn().mockResolvedValue(undefined),
  release: vi.fn()
}))
vi.mock('../../git/runner', () => ({
  ghExecFileAsync: ghExecFileAsyncMock,
  extractExecError: () => ({ stderr: '', stdout: '' })
}))
vi.mock('../rate-limit', () => ({
  rateLimitGuard: () => ({ blocked: false }),
  noteRateLimitSpend: vi.fn()
}))

import { runGraphql, runRest } from './internals'

describe('projects gh host pinning through internals (#1715)', () => {
  beforeEach(() => {
    ghExecFileAsyncMock.mockReset().mockResolvedValue({
      stdout: JSON.stringify({ data: { ok: true } }),
      stderr: ''
    })
  })

  it('runGraphql pins --hostname for a GHES host', async () => {
    await runGraphql<unknown>('query { viewer { login } }', {}, undefined, 'ghe.corp.example')
    const args = ghExecFileAsyncMock.mock.calls[0][0] as string[]
    expect(args.slice(0, 4)).toEqual(['api', 'graphql', '--hostname', 'ghe.corp.example'])
  })

  it('runGraphql omits --hostname for the default host', async () => {
    await runGraphql<unknown>('query { viewer { login } }', {}, undefined, null)
    const args = ghExecFileAsyncMock.mock.calls[0][0] as string[]
    expect(args).not.toContain('--hostname')
  })

  it('runRest pins --hostname before the endpoint args', async () => {
    await runRest<unknown>(['-X', 'GET', 'repos/acme/app/labels'], undefined, 'core', {
      host: 'ghe.corp.example'
    })
    const args = ghExecFileAsyncMock.mock.calls[0][0] as string[]
    expect(args.slice(0, 3)).toEqual(['api', '--hostname', 'ghe.corp.example'])
  })

  it('runRest omits --hostname without a host option', async () => {
    await runRest<unknown>(['-X', 'GET', 'repos/acme/app/labels'])
    const args = ghExecFileAsyncMock.mock.calls[0][0] as string[]
    expect(args).not.toContain('--hostname')
  })
})
