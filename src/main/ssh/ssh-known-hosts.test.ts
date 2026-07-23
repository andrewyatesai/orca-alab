import { describe, expect, it } from 'vitest'
import { createHmac } from 'node:crypto'
import {
  buildHostTokens,
  classifyPresentedHostKey,
  computeHostKeyFingerprint,
  hostFieldMatches,
  parseHostKeyType,
  parseKnownHosts,
  type KnownHostEntry
} from './ssh-known-hosts'

// Build a valid SSH wire-format host key: length-prefixed algorithm name + body bytes.
function wireKey(type: string, body: string): Buffer {
  const typeBuf = Buffer.from(type, 'ascii')
  const len = Buffer.alloc(4)
  len.writeUInt32BE(typeBuf.length)
  return Buffer.concat([len, typeBuf, Buffer.from(body, 'utf8')])
}

function entry(
  hostField: string,
  key: Buffer,
  overrides?: Partial<KnownHostEntry>
): KnownHostEntry {
  return {
    hostField,
    keyType: parseHostKeyType(key),
    keyBase64: key.toString('base64'),
    ...overrides
  }
}

describe('parseHostKeyType', () => {
  it('reads the algorithm name from a wire-format key', () => {
    expect(parseHostKeyType(wireKey('ssh-ed25519', 'abc'))).toBe('ssh-ed25519')
    expect(parseHostKeyType(wireKey('ecdsa-sha2-nistp256', 'xyz'))).toBe('ecdsa-sha2-nistp256')
  })

  it('returns empty for buffers too short or with a bogus length prefix', () => {
    expect(parseHostKeyType(Buffer.from([0x00, 0x01]))).toBe('')
    expect(parseHostKeyType(Buffer.from('not-a-real-wire-key'))).toBe('')
  })
})

describe('computeHostKeyFingerprint', () => {
  it('produces an unpadded SHA256 base64 fingerprint', () => {
    expect(computeHostKeyFingerprint(wireKey('ssh-ed25519', 'a'))).toMatch(
      /^SHA256:[A-Za-z\d+/]{43}$/
    )
  })
})

describe('buildHostTokens', () => {
  it('uses a bare host on port 22 and [host]:port otherwise', () => {
    expect(buildHostTokens('example.com', 22)).toEqual(['example.com'])
    expect(buildHostTokens('example.com', 2222)).toEqual(['[example.com]:2222'])
  })
})

describe('hostFieldMatches', () => {
  it('matches comma lists and wildcard patterns', () => {
    expect(hostFieldMatches('a.com,b.com', ['b.com'])).toBe(true)
    expect(hostFieldMatches('*.example.com', ['host.example.com'])).toBe(true)
    expect(hostFieldMatches('*.example.com', ['example.com'])).toBe(false)
  })

  it('honors negation as an explicit exclusion', () => {
    expect(hostFieldMatches('*.example.com,!secret.example.com', ['secret.example.com'])).toBe(
      false
    )
  })

  it('matches hashed (HashKnownHosts) entries', () => {
    const salt = Buffer.from('0123456789abcdef0123')
    const token = 'example.com'
    const hash = createHmac('sha1', salt).update(token).digest('base64')
    const field = `|1|${salt.toString('base64')}|${hash}`
    expect(hostFieldMatches(field, [token])).toBe(true)
    expect(hostFieldMatches(field, ['other.com'])).toBe(false)
  })
})

describe('parseKnownHosts', () => {
  it('skips comments and blank lines and records markers', () => {
    const key = wireKey('ssh-ed25519', 'k')
    const content = [
      '# a comment',
      '',
      `example.com ssh-ed25519 ${key.toString('base64')}`,
      `@revoked bad.com ssh-ed25519 ${key.toString('base64')}`
    ].join('\n')
    const parsed = parseKnownHosts(content)
    expect(parsed).toHaveLength(2)
    expect(parsed[0]).toMatchObject({ hostField: 'example.com', keyType: 'ssh-ed25519' })
    expect(parsed[1].marker).toBe('@revoked')
  })

  it('ignores lines missing a key field', () => {
    expect(parseKnownHosts('example.com ssh-ed25519')).toEqual([])
  })
})

describe('classifyPresentedHostKey', () => {
  const trusted = wireKey('ssh-ed25519', 'trusted-key-body')
  const attacker = wireKey('ssh-ed25519', 'attacker-key-body')

  it('returns match when the pinned key is presented', () => {
    const entries = [entry('example.com', trusted)]
    expect(classifyPresentedHostKey(entries, 'example.com', 22, trusted)).toBe('match')
  })

  it('returns mismatch when a different key is presented for a known host+algorithm (MITM)', () => {
    const entries = [entry('example.com', trusted)]
    expect(classifyPresentedHostKey(entries, 'example.com', 22, attacker)).toBe('mismatch')
  })

  it('returns unknown for a host with no matching entry', () => {
    const entries = [entry('other.com', trusted)]
    expect(classifyPresentedHostKey(entries, 'example.com', 22, trusted)).toBe('unknown')
  })

  it('returns unknown when only a different algorithm is pinned for the host', () => {
    const ecdsa = wireKey('ecdsa-sha2-nistp256', 'ec-body')
    const entries = [entry('example.com', ecdsa)]
    expect(classifyPresentedHostKey(entries, 'example.com', 22, trusted)).toBe('unknown')
  })

  it('matches a host on a non-standard port via [host]:port', () => {
    const entries = [entry('[example.com]:2222', trusted)]
    expect(classifyPresentedHostKey(entries, 'example.com', 2222, trusted)).toBe('match')
    expect(classifyPresentedHostKey(entries, 'example.com', 22, trusted)).toBe('unknown')
  })

  it('treats a @revoked key as a mismatch', () => {
    const entries = [entry('example.com', trusted, { marker: '@revoked' })]
    expect(classifyPresentedHostKey(entries, 'example.com', 22, trusted)).toBe('mismatch')
  })

  it('ignores @cert-authority anchors (no CA validation)', () => {
    const entries = [entry('*.example.com', trusted, { marker: '@cert-authority' })]
    expect(classifyPresentedHostKey(entries, 'host.example.com', 22, trusted)).toBe('unknown')
  })
})
