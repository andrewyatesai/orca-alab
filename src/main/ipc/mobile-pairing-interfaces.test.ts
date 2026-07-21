import { describe, expect, it } from 'vitest'
import {
  getDefaultPairingAddress,
  listPairingNetworkInterfaces
} from './mobile-pairing-interfaces'
import type { networkInterfaces } from 'node:os'

// Why: only the fields the enumeration reads; os.NetworkInterfaceInfo carries
// netmask/mac/cidr the pairing logic never touches.
function osInterfaces(
  value: Record<string, { family: 'IPv4' | 'IPv6'; internal: boolean; address: string }[]>
): typeof networkInterfaces {
  return (() => value) as unknown as typeof networkInterfaces
}

const isTailnetIPv4 = (address: string): boolean => address.startsWith('100.')

describe('listPairingNetworkInterfaces', () => {
  it('advertises global IPv6 addresses on IPv6-only hosts', () => {
    const deps = {
      readInterfaces: osInterfaces({
        en0: [{ family: 'IPv6', internal: false, address: '2001:db8::1234' }]
      }),
      isTailnetIPv4
    }
    expect(listPairingNetworkInterfaces(deps)).toEqual([
      { name: 'en0', address: '2001:db8::1234' }
    ])
    expect(getDefaultPairingAddress(deps)).toBe('2001:db8::1234')
  })

  it('excludes internal and link-local addresses', () => {
    expect(
      listPairingNetworkInterfaces({
        readInterfaces: osInterfaces({
          lo0: [
            { family: 'IPv4', internal: true, address: '127.0.0.1' },
            { family: 'IPv6', internal: true, address: '::1' }
          ],
          en0: [{ family: 'IPv6', internal: false, address: 'fe80::abcd' }]
        }),
        isTailnetIPv4
      })
    ).toEqual([])
  })

  it('orders tailnet overlay first, then LAN IPv4, then other IPv6', () => {
    expect(
      listPairingNetworkInterfaces({
        readInterfaces: osInterfaces({
          en0: [
            { family: 'IPv6', internal: false, address: '2001:db8::77' },
            { family: 'IPv4', internal: false, address: '192.168.1.24' }
          ],
          tailscale0: [
            { family: 'IPv6', internal: false, address: 'fd7a:115c:a1e0::9' },
            { family: 'IPv4', internal: false, address: '100.64.1.20' }
          ]
        }),
        isTailnetIPv4
      })
    ).toEqual([
      { name: 'tailscale0', address: 'fd7a:115c:a1e0::9' },
      { name: 'tailscale0', address: '100.64.1.20' },
      { name: 'en0', address: '192.168.1.24' },
      { name: 'en0', address: '2001:db8::77' }
    ])
  })

  it('returns a null default address when no interface qualifies', () => {
    expect(getDefaultPairingAddress({ readInterfaces: osInterfaces({}), isTailnetIPv4 })).toBeNull()
  })
})
