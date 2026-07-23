// Why: the E2EE keypair enables application-layer encryption between mobile
// and desktop over plain ws://. The public key is embedded in the QR pairing
// offer so the mobile client can derive a shared secret via ECDH.
import { existsSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { safeStorage } from 'electron'
import { generateKeyPair } from '../../shared/e2ee-crypto'
import { hardenExistingSecureFile, writeSecureJsonFile } from '../../shared/secure-file'
import { E2EE_KEYPAIR_FILENAME } from './mobile-pairing-files'

const KEYPAIR_FILENAME = E2EE_KEYPAIR_FILENAME
const KEYPAIR_VERSION = 2
const MAX_KEYPAIR_FILE_BYTES = 8 * 1024

// Why: the secret half is this desktop's sole confidentiality anchor for all
// mobile E2EE, so wrap it in the OS keychain (mirroring the cloud-session
// store) rather than persisting the raw base64 that 0600 alone protected.
type EncryptedKeypairFile = {
  v: 2
  publicKeyB64: string
  secretKeyFormat: 'electron-safe-storage-v1'
  secretKeyCiphertextB64: string
}

type PlaintextKeypairFile = {
  v: 2
  publicKeyB64: string
  secretKeyFormat: 'plaintext'
  secretKeyB64: string
}

// Why: pre-encryption files stored the raw secret at { v: 1 }; keep reading them
// so existing pairings survive, then migrate to the encrypted envelope on load.
type LegacyKeypairFile = {
  v: 1
  publicKeyB64: string
  secretKeyB64: string
}

type KeypairFile = EncryptedKeypairFile | PlaintextKeypairFile | LegacyKeypairFile

export type E2EEKeypair = {
  publicKey: Uint8Array
  secretKey: Uint8Array
  publicKeyB64: string
}

// Why: in the vitest node runtime `electron` resolves to a path string, so
// safeStorage is undefined; treat any failure as "unavailable" and fall back.
function isEncryptionAvailable(): boolean {
  try {
    return safeStorage?.isEncryptionAvailable() === true
  } catch {
    return false
  }
}

function buildKeypairFile(publicKeyB64: string, secretKeyB64: string): KeypairFile {
  if (isEncryptionAvailable()) {
    return {
      v: KEYPAIR_VERSION,
      publicKeyB64,
      secretKeyFormat: 'electron-safe-storage-v1',
      secretKeyCiphertextB64: safeStorage.encryptString(secretKeyB64).toString('base64')
    }
  }
  return { v: KEYPAIR_VERSION, publicKeyB64, secretKeyFormat: 'plaintext', secretKeyB64 }
}

// Why: returns null when the secret cannot be recovered (unknown format or a
// keychain that can no longer decrypt) so the caller regenerates the keypair.
function decodeSecretKeyB64(
  raw: KeypairFile
): { secretKeyB64: string; wasPlaintext: boolean } | null {
  if (raw.v === 1) {
    return { secretKeyB64: raw.secretKeyB64, wasPlaintext: true }
  }
  if (raw.v === KEYPAIR_VERSION) {
    if (raw.secretKeyFormat === 'plaintext') {
      return { secretKeyB64: raw.secretKeyB64, wasPlaintext: true }
    }
    if (raw.secretKeyFormat === 'electron-safe-storage-v1') {
      if (!isEncryptionAvailable()) {
        return null
      }
      const secretKeyB64 = safeStorage.decryptString(
        Buffer.from(raw.secretKeyCiphertextB64, 'base64')
      )
      return { secretKeyB64, wasPlaintext: false }
    }
  }
  return null
}

export function loadOrCreateE2EEKeypair(userDataPath: string): E2EEKeypair {
  const filePath = join(userDataPath, KEYPAIR_FILENAME)

  if (existsSync(filePath)) {
    try {
      hardenExistingSecureFile(filePath)
      // Why: this startup path reads synchronously; valid keypair files are
      // tiny, so oversized/corrupt files should be replaced without loading.
      if (statSync(filePath).size > MAX_KEYPAIR_FILE_BYTES) {
        throw new Error('E2EE keypair file is too large')
      }
      const raw: KeypairFile = JSON.parse(readFileSync(filePath, 'utf-8'))
      const decoded = raw?.publicKeyB64 ? decodeSecretKeyB64(raw) : null
      if (decoded) {
        const publicKey = Uint8Array.from(Buffer.from(raw.publicKeyB64, 'base64'))
        const secretKey = Uint8Array.from(Buffer.from(decoded.secretKeyB64, 'base64'))
        if (publicKey.length === 32 && secretKey.length === 32) {
          // Why: upgrade legacy/plaintext-on-disk secrets to the encrypted
          // envelope once the keychain is available, so at-rest exposure closes.
          if (decoded.wasPlaintext && isEncryptionAvailable()) {
            try {
              writeSecureJsonFile(
                filePath,
                buildKeypairFile(raw.publicKeyB64, decoded.secretKeyB64)
              )
            } catch {
              // Migration is best-effort; the loaded keypair is still valid.
            }
          }
          return { publicKey, secretKey, publicKeyB64: raw.publicKeyB64 }
        }
      }
    } catch {
      // Malformed or undecryptable file — regenerate below.
    }
  }

  const keypair = generateKeyPair()
  const publicKeyB64 = Buffer.from(keypair.publicKey).toString('base64')
  const secretKeyB64 = Buffer.from(keypair.secretKey).toString('base64')

  writeSecureJsonFile(filePath, buildKeypairFile(publicKeyB64, secretKeyB64))

  return { publicKey: keypair.publicKey, secretKey: keypair.secretKey, publicKeyB64 }
}
