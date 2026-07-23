import { createHash, createHmac } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join, delimiter } from 'node:path'

// Why: OpenSSH marks host lines that carry a CA public key or a revoked key.
type KnownHostMarker = '@cert-authority' | '@revoked'

export type KnownHostEntry = {
  marker?: KnownHostMarker
  /** Raw host field: plain name, `[host]:port`, comma list, or hashed `|1|salt|hash`. */
  hostField: string
  keyType: string
  keyBase64: string
}

export type HostKeyVerdict = 'match' | 'mismatch' | 'unknown'

// Why: SSH host keys arrive in wire format — a length-prefixed algorithm name followed by key data.
export function parseHostKeyType(key: Buffer): string {
  if (key.length < 4) {
    return ''
  }
  const len = key.readUInt32BE(0)
  // Algorithm names are short ASCII ("ssh-ed25519", "ecdsa-sha2-nistp521"); reject bogus lengths.
  if (len <= 0 || len > 64 || key.length < 4 + len) {
    return ''
  }
  return key.subarray(4, 4 + len).toString('ascii')
}

export function computeHostKeyFingerprint(key: Buffer): string {
  const digest = createHash('sha256').update(key).digest('base64').replace(/=+$/, '')
  return `SHA256:${digest}`
}

// Why: OpenSSH keys a host by bare name on port 22 and by `[host]:port` otherwise.
export function buildHostTokens(host: string, port: number): string[] {
  const normalized = host.trim()
  if (!normalized) {
    return []
  }
  return port === 22 ? [normalized] : [`[${normalized}]:${port}`]
}

function matchesWildcard(pattern: string, token: string): boolean {
  // Translate OpenSSH glob (* and ?) to an anchored, case-insensitive regexp.
  const escaped = pattern.replace(/[.+^${}()|[\]\\]/g, '\\$&')
  const regexBody = escaped.replace(/\*/g, '.*').replace(/\?/g, '.')
  return new RegExp(`^${regexBody}$`, 'i').test(token)
}

function hashedFieldMatches(hostField: string, tokens: string[]): boolean {
  // Format: |1|<base64 salt>|<base64 HMAC-SHA1(salt, token)>
  const parts = hostField.split('|')
  if (parts.length !== 4 || parts[1] !== '1') {
    return false
  }
  const salt = Buffer.from(parts[2], 'base64')
  const expected = parts[3]
  return tokens.some(
    (token) => createHmac('sha1', salt).update(token).digest('base64') === expected
  )
}

// Returns true if the entry's host field covers any candidate token; false if a
// negated pattern (`!host`) explicitly excludes it.
export function hostFieldMatches(hostField: string, tokens: string[]): boolean {
  if (hostField.startsWith('|1|')) {
    return hashedFieldMatches(hostField, tokens)
  }
  let matched = false
  for (const rawPattern of hostField.split(',')) {
    const negated = rawPattern.startsWith('!')
    const pattern = negated ? rawPattern.slice(1) : rawPattern
    if (!pattern) {
      continue
    }
    const hit = tokens.some((token) => matchesWildcard(pattern, token))
    if (hit) {
      // A negated pattern is an explicit exclusion and wins over any positive match.
      if (negated) {
        return false
      }
      matched = true
    }
  }
  return matched
}

export function parseKnownHosts(content: string): KnownHostEntry[] {
  const entries: KnownHostEntry[] = []
  for (const rawLine of content.split(/\r?\n/)) {
    const line = rawLine.trim()
    if (!line || line.startsWith('#')) {
      continue
    }
    const tokens = line.split(/\s+/)
    let index = 0
    let marker: KnownHostMarker | undefined
    const first = tokens[index]
    if (first === '@cert-authority' || first === '@revoked') {
      marker = first
      index += 1
    }
    const hostField = tokens[index]
    const keyType = tokens[index + 1]
    const keyBase64 = tokens[index + 2]
    if (!hostField || !keyType || !keyBase64) {
      continue
    }
    entries.push({ marker, hostField, keyType, keyBase64 })
  }
  return entries
}

// Decides how a presented host key relates to what is already known for this host:
// - 'match'    : an entry for this host+algorithm holds exactly this key.
// - 'mismatch' : an entry for this host+algorithm holds a DIFFERENT key, or a
//                matching key is @revoked. Rejecting protects against MITM/changed keys.
// - 'unknown'  : no entry pins this host+algorithm (first contact).
export function classifyPresentedHostKey(
  entries: KnownHostEntry[],
  host: string,
  port: number,
  key: Buffer
): HostKeyVerdict {
  const keyType = parseHostKeyType(key)
  const keyBase64 = key.toString('base64')
  const tokens = buildHostTokens(host, port)
  if (tokens.length === 0) {
    return 'unknown'
  }
  let sawSameAlgorithm = false
  for (const entry of entries) {
    // Skip CA anchors: we do not implement certificate validation, so they neither pin nor reject.
    if (entry.marker === '@cert-authority') {
      continue
    }
    if (entry.keyType !== keyType || !hostFieldMatches(entry.hostField, tokens)) {
      continue
    }
    if (entry.marker === '@revoked') {
      if (entry.keyBase64 === keyBase64) {
        return 'mismatch'
      }
      continue
    }
    sawSameAlgorithm = true
    if (entry.keyBase64 === keyBase64) {
      return 'match'
    }
  }
  return sawSameAlgorithm ? 'mismatch' : 'unknown'
}

// Why: honor the user's real trust store first, then Orca's; an env override keeps tests off the real files.
export function knownHostsPaths(): string[] {
  const override = process.env.ORCA_SSH_KNOWN_HOSTS_PATH
  if (override) {
    return override.split(delimiter).filter(Boolean)
  }
  const sshDir = join(homedir(), '.ssh')
  return [
    join(sshDir, 'known_hosts'),
    join(sshDir, 'known_hosts2'),
    join('/etc', 'ssh', 'ssh_known_hosts')
  ]
}

export function readKnownHostsEntries(paths: string[] = knownHostsPaths()): KnownHostEntry[] {
  const entries: KnownHostEntry[] = []
  for (const path of paths) {
    try {
      entries.push(...parseKnownHosts(readFileSync(path, 'utf8')))
    } catch {
      // Missing/unreadable known_hosts files are expected; skip them.
    }
  }
  return entries
}
