import { app, safeStorage } from 'electron'
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'

type StoredOpenAiKey = {
  encryptedKeyBase64: string
}

const OPENAI_SPEECH_TOKEN_FILE = 'openai-speech-token.enc'
// Why: an honest name — a cleartext fallback must not masquerade as encrypted (`.enc`) on disk.
const OPENAI_SPEECH_PLAINTEXT_FILE = 'openai-speech-token.plaintext'
// Why: gives headless/SSH hosts a non-plaintext way to supply the key without ever touching disk.
const OPENAI_SPEECH_API_KEY_ENV = 'ORCA_OPENAI_SPEECH_API_KEY'
let cachedOpenAiSpeechApiKey: string | null = null

function getOrcaDir(): string {
  return join(homedir(), '.orca')
}

function ensureOrcaDir(): void {
  const dir = getOrcaDir()
  if (!existsSync(dir)) {
    mkdirSync(dir, { recursive: true })
  }
}

function getOpenAiKeyPath(): string {
  return join(getOrcaDir(), OPENAI_SPEECH_TOKEN_FILE)
}

function getOpenAiPlaintextKeyPath(): string {
  return join(getOrcaDir(), OPENAI_SPEECH_PLAINTEXT_FILE)
}

function envProvidedKey(): string | null {
  const value = process.env[OPENAI_SPEECH_API_KEY_ENV]?.trim()
  return value ? value : null
}

// Why: a secret must never be silently written in cleartext — the fork targets headless/SSH Linux hosts
// where safeStorage is routinely unavailable. A dev may opt in explicitly (non-prod, unpackaged), mirroring
// persistence.ts's allowsPlaintextPersistedSecret so the whole app shares one opt-in flag.
function allowsPlaintextSpeechKey(env: NodeJS.ProcessEnv = process.env): boolean {
  let packaged = false
  try {
    packaged = app?.isPackaged === true
  } catch {
    packaged = false
  }
  return (
    env.ORCA_ALLOW_PLAINTEXT_PERSISTED_SECRETS === '1' && env.NODE_ENV !== 'production' && !packaged
  )
}

function readLegacyJsonStoredOpenAiKey(): StoredOpenAiKey | null {
  const keyPath = getOpenAiKeyPath()
  if (!existsSync(keyPath)) {
    return null
  }
  try {
    const parsed = JSON.parse(readFileSync(keyPath, 'utf8')) as Partial<StoredOpenAiKey>
    if (typeof parsed.encryptedKeyBase64 !== 'string' || parsed.encryptedKeyBase64 === '') {
      return null
    }
    return { encryptedKeyBase64: parsed.encryptedKeyBase64 }
  } catch {
    return null
  }
}

export function hasOpenAiSpeechApiKey(): boolean {
  // Why: Settings and model-state refresh call this on startup; checking file
  // existence avoids decrypting safeStorage and triggering macOS keychain prompts.
  if (envProvidedKey() !== null || cachedOpenAiSpeechApiKey !== null) {
    return true
  }
  return existsSync(getOpenAiKeyPath()) || existsSync(getOpenAiPlaintextKeyPath())
}

export function saveOpenAiSpeechApiKey(apiKey: string): void {
  const trimmed = apiKey.trim()
  if (!trimmed) {
    throw new Error('OpenAI API key is required')
  }
  ensureOrcaDir()
  if (safeStorage.isEncryptionAvailable()) {
    writeFileSync(getOpenAiKeyPath(), safeStorage.encryptString(trimmed), { mode: 0o600 })
    // Why: a stale plaintext fallback must not linger and shadow the freshly encrypted key.
    rmSync(getOpenAiPlaintextKeyPath(), { force: true })
    cachedOpenAiSpeechApiKey = trimmed
    return
  }

  if (!allowsPlaintextSpeechKey()) {
    // Why: keep the key in memory so the running session still works, but refuse to write cleartext.
    cachedOpenAiSpeechApiKey = trimmed
    throw new Error(
      'OpenAI API key cannot be stored securely: OS encryption (safeStorage) is unavailable. ' +
        `Unlock your login keyring, or provide the key via the ${OPENAI_SPEECH_API_KEY_ENV} environment variable.`
    )
  }

  console.warn(
    '[speech] safeStorage unavailable and ORCA_ALLOW_PLAINTEXT_PERSISTED_SECRETS opt-in set — storing OpenAI speech key in plaintext'
  )
  writeFileSync(getOpenAiPlaintextKeyPath(), trimmed, { encoding: 'utf8', mode: 0o600 })
  // Why: never leave a `.enc`-named file holding cleartext — remove any prior encrypted key.
  rmSync(getOpenAiKeyPath(), { force: true })
  cachedOpenAiSpeechApiKey = trimmed
}

export function readOpenAiSpeechApiKey(): string {
  const envKey = envProvidedKey()
  if (envKey !== null) {
    return envKey
  }
  if (cachedOpenAiSpeechApiKey !== null) {
    return cachedOpenAiSpeechApiKey
  }

  const keyPath = getOpenAiKeyPath()
  if (existsSync(keyPath)) {
    try {
      const raw = readFileSync(keyPath)
      const legacyJson = readLegacyJsonStoredOpenAiKey()
      if (legacyJson) {
        cachedOpenAiSpeechApiKey = safeStorage.decryptString(
          Buffer.from(legacyJson.encryptedKeyBase64, 'base64')
        )
        return cachedOpenAiSpeechApiKey
      }
      // Why: legacy installs wrote cleartext to the `.enc` path when encryption was unavailable; still read it.
      cachedOpenAiSpeechApiKey = safeStorage.isEncryptionAvailable()
        ? safeStorage.decryptString(raw)
        : raw.toString('utf8')
      return cachedOpenAiSpeechApiKey
    } catch {
      throw new Error('OpenAI API key could not be decrypted')
    }
  }

  const plaintextPath = getOpenAiPlaintextKeyPath()
  if (existsSync(plaintextPath)) {
    cachedOpenAiSpeechApiKey = readFileSync(plaintextPath, 'utf8')
    return cachedOpenAiSpeechApiKey
  }

  throw new Error('OpenAI API key is not configured')
}

export function clearOpenAiSpeechApiKey(): void {
  cachedOpenAiSpeechApiKey = null
  rmSync(getOpenAiKeyPath(), { force: true })
  rmSync(getOpenAiPlaintextKeyPath(), { force: true })
}
