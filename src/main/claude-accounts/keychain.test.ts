import { createHash } from 'node:crypto'
import { execFile } from 'node:child_process'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  deleteActiveClaudeKeychainCredentials,
  readActiveClaudeKeychainCredentials,
  readActiveClaudeKeychainCredentialsStrict,
  writeActiveClaudeKeychainCredentials,
  writeActiveClaudeKeychainCredentialsForRuntime
} from './keychain'

vi.mock('node:child_process', () => ({
  execFile: vi.fn()
}))

const execFileMock = vi.mocked(execFile)
const originalPlatform = Object.getOwnPropertyDescriptor(process, 'platform')

function setPlatform(platform: NodeJS.Platform): void {
  Object.defineProperty(process, 'platform', {
    configurable: true,
    value: platform
  })
}

function serviceForConfigDir(configDir: string): string {
  const suffix = createHash('sha256').update(configDir).digest('hex').slice(0, 8)
  return `Claude Code-credentials-${suffix}`
}

function invokeExecFileCallback(
  callback: unknown,
  error: Error | null,
  stdout: string,
  stderr: string
): void {
  const execCallback = callback as (error: Error | null, stdout: string, stderr: string) => void
  execCallback(error, stdout, stderr)
}

type StdinCapture = { written: string; end: ReturnType<typeof vi.fn> }

// Returns a fake ChildProcess whose stdin records what the caller writes, so
// tests can assert secrets travel over stdin rather than argv.
function fakeChildWithStdin(capture: StdinCapture): { stdin: unknown } {
  return {
    stdin: {
      on: vi.fn(),
      end: capture.end.mockImplementation((chunk?: string) => {
        if (typeof chunk === 'string') {
          capture.written += chunk
        }
      })
    }
  }
}

describe('Claude Keychain credentials', () => {
  beforeEach(() => {
    setPlatform('darwin')
    execFileMock.mockReset()
  })

  afterEach(() => {
    vi.useRealTimers()
    if (originalPlatform) {
      Object.defineProperty(process, 'platform', originalPlatform)
    }
  })

  it('reads config-scoped Claude Code 2.1 credentials before legacy credentials', async () => {
    const configDir = '/tmp/orca-claude-login-test'
    const scopedService = serviceForConfigDir(configDir)
    execFileMock.mockImplementationOnce((_file, _args, _options, callback) => {
      invokeExecFileCallback(callback, null, '{"claudeAiOauth":{"accessToken":"scoped"}}\n', '')
      return null as never
    })

    await expect(readActiveClaudeKeychainCredentials(configDir)).resolves.toBe(
      '{"claudeAiOauth":{"accessToken":"scoped"}}'
    )

    expect(execFileMock).toHaveBeenCalledTimes(1)
    expect(execFileMock.mock.calls[0][1]).toEqual([
      'find-generic-password',
      '-s',
      scopedService,
      '-a',
      process.env.USER || process.env.USERNAME || 'user',
      '-w'
    ])
  })

  it('falls back to the legacy unsuffixed Claude Code credentials service', async () => {
    const configDir = '/tmp/orca-claude-login-test'
    const notFound = Object.assign(new Error('not found'), { code: 44 })
    execFileMock
      .mockImplementationOnce((_file, _args, _options, callback) => {
        invokeExecFileCallback(callback, notFound, '', 'could not be found')
        return null as never
      })
      .mockImplementationOnce((_file, _args, _options, callback) => {
        invokeExecFileCallback(callback, null, 'legacy\n', '')
        return null as never
      })

    await expect(readActiveClaudeKeychainCredentials(configDir)).resolves.toBe('legacy')

    expect(execFileMock.mock.calls[1][1]).toEqual([
      'find-generic-password',
      '-s',
      'Claude Code-credentials',
      '-a',
      process.env.USER || process.env.USERNAME || 'user',
      '-w'
    ])
  })

  it('writes active credentials via stdin, never placing the secret on argv', async () => {
    const configDir = '/tmp/orca-claude-login-test'
    const scopedService = serviceForConfigDir(configDir)
    const capture: StdinCapture = { written: '', end: vi.fn() }
    execFileMock.mockImplementationOnce((_file, _args, _options, callback) => {
      invokeExecFileCallback(callback, null, '', '')
      return fakeChildWithStdin(capture) as never
    })

    await writeActiveClaudeKeychainCredentials('credentials-json', configDir)

    // The secret must not appear anywhere in argv (CWE-214 process-list leak).
    const argv = execFileMock.mock.calls[0][1] as string[]
    expect(argv).toEqual([
      'add-generic-password',
      '-U',
      '-s',
      scopedService,
      '-a',
      process.env.USER || process.env.USERNAME || 'user',
      '-w'
    ])
    expect(argv).not.toContain('credentials-json')
    // `security -w` prompts for the value plus a retype confirmation.
    expect(capture.written).toBe('credentials-json\ncredentials-json\n')
  })

  it('writes runtime credentials to scoped and legacy services for old Claude Code compatibility', async () => {
    const configDir = '/tmp/orca-claude-login-test'
    const scopedService = serviceForConfigDir(configDir)
    const captures: StdinCapture[] = []
    execFileMock.mockImplementation((_file, _args, _options, callback) => {
      const capture: StdinCapture = { written: '', end: vi.fn() }
      captures.push(capture)
      invokeExecFileCallback(callback, null, '', '')
      return fakeChildWithStdin(capture) as never
    })

    await writeActiveClaudeKeychainCredentialsForRuntime('credentials-json', configDir)

    // Both services use the -w prompt (no argv secret) and receive the value
    // twice via stdin (value + retype confirmation).
    expect(execFileMock.mock.calls.map((call) => call[1])).toEqual([
      [
        'add-generic-password',
        '-U',
        '-s',
        scopedService,
        '-a',
        process.env.USER || process.env.USERNAME || 'user',
        '-w'
      ],
      [
        'add-generic-password',
        '-U',
        '-s',
        'Claude Code-credentials',
        '-a',
        process.env.USER || process.env.USERNAME || 'user',
        '-w'
      ]
    ])
    expect(captures.map((capture) => capture.written)).toEqual([
      'credentials-json\ncredentials-json\n',
      'credentials-json\ncredentials-json\n'
    ])
  })

  it('strictly reads only the requested active credentials service', async () => {
    const configDir = '/tmp/orca-claude-login-test'
    const scopedService = serviceForConfigDir(configDir)
    execFileMock.mockImplementationOnce((_file, _args, _options, callback) => {
      invokeExecFileCallback(callback, null, 'scoped\n', '')
      return null as never
    })

    await expect(readActiveClaudeKeychainCredentialsStrict(configDir)).resolves.toBe('scoped')

    expect(execFileMock).toHaveBeenCalledTimes(1)
    expect(execFileMock.mock.calls[0][1]).toEqual([
      'find-generic-password',
      '-s',
      scopedService,
      '-a',
      process.env.USER || process.env.USERNAME || 'user',
      '-w'
    ])
  })

  it('rejects when a keychain read never reports completion', async () => {
    vi.useFakeTimers()
    const configDir = '/tmp/orca-claude-login-test'
    const killMock = vi.fn()
    execFileMock.mockImplementationOnce(() => ({ kill: killMock }) as never)

    let settled = false
    let rejected: unknown
    const readPromise = readActiveClaudeKeychainCredentialsStrict(configDir).then(
      (credentials) => {
        settled = true
        return credentials
      },
      (error: unknown) => {
        settled = true
        rejected = error
        return null
      }
    )

    await vi.advanceTimersByTimeAsync(3000)

    expect(settled).toBe(true)
    await readPromise
    expect(rejected).toEqual(
      expect.objectContaining({ message: 'security timed out after 3000ms' })
    )
    expect(killMock).toHaveBeenCalled()
  })

  it('deletes both scoped and legacy active credentials for config-dir cleanup', async () => {
    const configDir = '/tmp/orca-claude-login-test'
    const scopedService = serviceForConfigDir(configDir)
    execFileMock.mockImplementation((_file, _args, _options, callback) => {
      invokeExecFileCallback(callback, null, '', '')
      return null as never
    })

    await deleteActiveClaudeKeychainCredentials(configDir)

    expect(execFileMock.mock.calls.map((call) => call[1])).toEqual([
      [
        'delete-generic-password',
        '-s',
        scopedService,
        '-a',
        process.env.USER || process.env.USERNAME || 'user'
      ],
      [
        'delete-generic-password',
        '-s',
        'Claude Code-credentials',
        '-a',
        process.env.USER || process.env.USERNAME || 'user'
      ]
    ])
  })
})
