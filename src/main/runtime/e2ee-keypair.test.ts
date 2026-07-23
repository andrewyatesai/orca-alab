import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { E2EE_KEYPAIR_FILENAME } from './mobile-pairing-files'

// Why: in the vitest node runtime `electron` resolves to a path string, so the
// real safeStorage is undefined; mock it so both the available and unavailable
// (headless/SSH) at-rest paths are exercised deterministically.
const safeStorageControl = vi.hoisted(() => ({ available: true }))
const safeStorageMock = vi.hoisted(() => ({
  isEncryptionAvailable: vi.fn(() => safeStorageControl.available),
  encryptString: vi.fn((plaintext: string) => Buffer.from(`enc:${plaintext}`, 'utf-8')),
  decryptString: vi.fn((buf: Buffer) => buf.toString('utf-8').replace(/^enc:/, ''))
}))

vi.mock('electron', () => ({ safeStorage: safeStorageMock }))

async function loadModule() {
  vi.resetModules()
  return import('./e2ee-keypair')
}

let dir = ''
beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'e2ee-keypair-'))
  safeStorageControl.available = true
  safeStorageMock.isEncryptionAvailable.mockClear()
  safeStorageMock.encryptString.mockClear()
  safeStorageMock.decryptString.mockClear()
})
afterEach(() => rmSync(dir, { recursive: true, force: true }))

const filePath = () => join(dir, E2EE_KEYPAIR_FILENAME)
const readFile = () => JSON.parse(readFileSync(filePath(), 'utf-8'))

describe('loadOrCreateE2EEKeypair', () => {
  it('persists a new secret encrypted at rest, never as raw base64', async () => {
    const { loadOrCreateE2EEKeypair } = await loadModule()
    const kp = loadOrCreateE2EEKeypair(dir)
    const onDisk = readFile()
    expect(onDisk.v).toBe(2)
    expect(onDisk.secretKeyFormat).toBe('electron-safe-storage-v1')
    expect(onDisk.secretKeyCiphertextB64).toBeTruthy()
    // Revert guard: the raw secret must not appear anywhere in the file.
    const rawSecretB64 = Buffer.from(kp.secretKey).toString('base64')
    expect(readFileSync(filePath(), 'utf-8')).not.toContain(rawSecretB64)
    expect('secretKeyB64' in onDisk).toBe(false)
  })

  it('round-trips: a reload returns the identical keypair via decrypt', async () => {
    const { loadOrCreateE2EEKeypair } = await loadModule()
    const first = loadOrCreateE2EEKeypair(dir)
    const second = loadOrCreateE2EEKeypair(dir)
    expect(Buffer.from(second.secretKey).toString('base64')).toBe(
      Buffer.from(first.secretKey).toString('base64')
    )
    expect(second.publicKeyB64).toBe(first.publicKeyB64)
    expect(safeStorageMock.decryptString).toHaveBeenCalled()
  })

  it('falls back to a plaintext envelope only when safeStorage is unavailable', async () => {
    safeStorageControl.available = false
    const { loadOrCreateE2EEKeypair } = await loadModule()
    loadOrCreateE2EEKeypair(dir)
    const onDisk = readFile()
    expect(onDisk.secretKeyFormat).toBe('plaintext')
    expect(safeStorageMock.encryptString).not.toHaveBeenCalled()
  })

  it('migrates a legacy v1 plaintext file to the encrypted envelope on load', async () => {
    // Seed a real keypair, capture its keys, then downgrade the file to v1 plaintext.
    const { loadOrCreateE2EEKeypair } = await loadModule()
    const seeded = loadOrCreateE2EEKeypair(dir)
    const secretKeyB64 = Buffer.from(seeded.secretKey).toString('base64')
    writeFileSync(
      filePath(),
      JSON.stringify({ v: 1, publicKeyB64: seeded.publicKeyB64, secretKeyB64 })
    )

    const loaded = loadOrCreateE2EEKeypair(dir)
    // Same keys recovered...
    expect(Buffer.from(loaded.secretKey).toString('base64')).toBe(secretKeyB64)
    // ...and the on-disk file was upgraded to the encrypted envelope.
    const onDisk = readFile()
    expect(onDisk.v).toBe(2)
    expect(onDisk.secretKeyFormat).toBe('electron-safe-storage-v1')
    expect(readFileSync(filePath(), 'utf-8')).not.toContain(secretKeyB64)
  })

  it('regenerates when an encrypted file cannot be decrypted', async () => {
    const { loadOrCreateE2EEKeypair } = await loadModule()
    const first = loadOrCreateE2EEKeypair(dir)
    const firstSecret = Buffer.from(first.secretKey).toString('base64')

    // Keychain became unavailable (rotation / restored profile): the encrypted
    // file can no longer be decrypted, so a fresh keypair must be minted.
    safeStorageControl.available = false
    const regenerated = loadOrCreateE2EEKeypair(dir)
    expect(Buffer.from(regenerated.secretKey).toString('base64')).not.toBe(firstSecret)
    expect(regenerated.secretKey.length).toBe(32)
  })
})
