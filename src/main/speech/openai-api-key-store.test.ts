import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import type * as Os from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const safeStorageMock = vi.hoisted(() => ({
  decryptString: vi.fn((value: Buffer) => value.toString('utf8')),
  encryptString: vi.fn((value: string) => Buffer.from(value)),
  isEncryptionAvailable: vi.fn(() => true)
}))

const appMock = vi.hoisted(() => ({ isPackaged: false }))

let tempHome = ''
const PLAINTEXT_OPT_IN_ENV = 'ORCA_ALLOW_PLAINTEXT_PERSISTED_SECRETS'
const DIRECT_KEY_ENV = 'ORCA_OPENAI_SPEECH_API_KEY'

async function loadStoreModule() {
  vi.resetModules()
  vi.doMock('electron', () => ({
    safeStorage: safeStorageMock,
    app: appMock
  }))
  vi.doMock('os', async () => {
    const actual = await vi.importActual<typeof Os>('os')
    return { ...actual, homedir: () => tempHome }
  })
  return import('./openai-api-key-store')
}

beforeEach(() => {
  tempHome = mkdtempLike('orca-openai-key-store-')
  safeStorageMock.decryptString.mockClear()
  safeStorageMock.encryptString.mockClear()
  safeStorageMock.isEncryptionAvailable.mockClear()
  safeStorageMock.isEncryptionAvailable.mockReturnValue(true)
  appMock.isPackaged = false
  delete process.env[PLAINTEXT_OPT_IN_ENV]
  delete process.env[DIRECT_KEY_ENV]
})

afterEach(() => {
  delete process.env[PLAINTEXT_OPT_IN_ENV]
  delete process.env[DIRECT_KEY_ENV]
})

function mkdtempLike(prefix: string): string {
  return mkdtempSync(join(tmpdir(), prefix))
}

function writeStoredOpenAiKey(value: string): void {
  const orcaDir = join(tempHome, '.orca')
  mkdirSync(orcaDir, { recursive: true })
  writeFileSync(join(orcaDir, 'openai-speech-token.enc'), value)
}

describe('OpenAI speech API key store', () => {
  it('checks configured status without decrypting or touching safeStorage', async () => {
    writeStoredOpenAiKey('encrypted-key')
    const store = await loadStoreModule()

    expect(store.hasOpenAiSpeechApiKey()).toBe(true)
    expect(safeStorageMock.isEncryptionAvailable).not.toHaveBeenCalled()
    expect(safeStorageMock.decryptString).not.toHaveBeenCalled()
  })

  it('decrypts only when the key is read for an API request', async () => {
    writeStoredOpenAiKey('encrypted-key')
    const store = await loadStoreModule()

    expect(store.readOpenAiSpeechApiKey()).toBe('encrypted-key')
    expect(safeStorageMock.decryptString).toHaveBeenCalledOnce()
  })

  it('caches the decrypted key so repeated dictations do not repeatedly touch safeStorage', async () => {
    writeStoredOpenAiKey('encrypted-key')
    const store = await loadStoreModule()

    expect(store.readOpenAiSpeechApiKey()).toBe('encrypted-key')
    expect(store.readOpenAiSpeechApiKey()).toBe('encrypted-key')
    expect(safeStorageMock.decryptString).toHaveBeenCalledOnce()
  })

  it('uses the in-memory key after save without decrypting from safeStorage', async () => {
    const store = await loadStoreModule()

    store.saveOpenAiSpeechApiKey('saved-key')

    expect(store.readOpenAiSpeechApiKey()).toBe('saved-key')
    expect(safeStorageMock.decryptString).not.toHaveBeenCalled()
  })

  it('reports missing status without creating storage files', async () => {
    const store = await loadStoreModule()

    expect(store.hasOpenAiSpeechApiKey()).toBe(false)
    expect(existsSync(join(tempHome, '.orca'))).toBe(false)
    expect(safeStorageMock.decryptString).not.toHaveBeenCalled()
  })

  it('refuses to write any key file when safeStorage is unavailable and no opt-in is set', async () => {
    safeStorageMock.isEncryptionAvailable.mockReturnValue(false)
    const store = await loadStoreModule()

    expect(() => store.saveOpenAiSpeechApiKey('sk-secret')).toThrow(/cannot be stored securely/)

    const orcaDir = join(tempHome, '.orca')
    expect(existsSync(join(orcaDir, 'openai-speech-token.enc'))).toBe(false)
    expect(existsSync(join(orcaDir, 'openai-speech-token.plaintext'))).toBe(false)
    // Why: the key stays in memory so the running session still works even though nothing was persisted.
    expect(store.readOpenAiSpeechApiKey()).toBe('sk-secret')
  })

  it('writes to a plaintext-named file (never .enc) only when the opt-in flag is set', async () => {
    safeStorageMock.isEncryptionAvailable.mockReturnValue(false)
    process.env[PLAINTEXT_OPT_IN_ENV] = '1'
    const store = await loadStoreModule()

    store.saveOpenAiSpeechApiKey('sk-optin')

    const orcaDir = join(tempHome, '.orca')
    // Cleartext must never land in a `.enc` file that implies encryption.
    expect(existsSync(join(orcaDir, 'openai-speech-token.enc'))).toBe(false)
    expect(existsSync(join(orcaDir, 'openai-speech-token.plaintext'))).toBe(true)
  })

  it('does not honor the plaintext opt-in when the app is packaged', async () => {
    safeStorageMock.isEncryptionAvailable.mockReturnValue(false)
    process.env[PLAINTEXT_OPT_IN_ENV] = '1'
    appMock.isPackaged = true
    const store = await loadStoreModule()

    expect(() => store.saveOpenAiSpeechApiKey('sk-secret')).toThrow(/cannot be stored securely/)
    expect(existsSync(join(tempHome, '.orca', 'openai-speech-token.plaintext'))).toBe(false)
  })

  it('reads an env-var-provided key without any file on disk', async () => {
    process.env[DIRECT_KEY_ENV] = 'sk-from-env'
    const store = await loadStoreModule()

    expect(store.hasOpenAiSpeechApiKey()).toBe(true)
    expect(store.readOpenAiSpeechApiKey()).toBe('sk-from-env')
    expect(existsSync(join(tempHome, '.orca'))).toBe(false)
    expect(safeStorageMock.decryptString).not.toHaveBeenCalled()
  })

  it('reads back an opt-in plaintext key from disk with a fresh in-memory cache', async () => {
    safeStorageMock.isEncryptionAvailable.mockReturnValue(false)
    process.env[PLAINTEXT_OPT_IN_ENV] = '1'
    const store = await loadStoreModule()
    store.saveOpenAiSpeechApiKey('sk-optin')

    // Reload the module so the cached key is gone and the read must come from disk.
    const fresh = await loadStoreModule()
    expect(fresh.readOpenAiSpeechApiKey()).toBe('sk-optin')
  })
})
