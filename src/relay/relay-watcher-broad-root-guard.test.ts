import { describe, expect, it } from 'vitest'
import { homedir } from 'node:os'
import { isBroadWatchRoot } from './relay-watcher-broad-root-guard'

describe('isBroadWatchRoot', () => {
  it('refuses the account home and filesystem roots', () => {
    expect(isBroadWatchRoot(homedir())).toBe(true)
    expect(isBroadWatchRoot(`${homedir()}/`)).toBe(true)
    expect(isBroadWatchRoot('/')).toBe(true)
    expect(isBroadWatchRoot('')).toBe(true)
  })

  it('refuses ancestors of home', () => {
    expect(isBroadWatchRoot('/home', '/home/dev')).toBe(true)
    expect(isBroadWatchRoot('/Users', '/Users/dev')).toBe(true)
  })

  it('refuses Windows drive roots and ancestors of a Windows home', () => {
    expect(isBroadWatchRoot('C:\\', 'C:\\Users\\dev')).toBe(true)
    expect(isBroadWatchRoot('C:/', 'C:\\Users\\dev')).toBe(true)
    expect(isBroadWatchRoot('C:\\Users', 'C:\\Users\\dev')).toBe(true)
    expect(isBroadWatchRoot('c:\\users\\dev', 'C:\\Users\\dev')).toBe(true)
    expect(isBroadWatchRoot('C:\\Users\\dev\\repo', 'C:\\Users\\dev')).toBe(false)
  })

  it('allows workspace-shaped roots under home', () => {
    expect(isBroadWatchRoot(`${homedir()}/projects/orca`)).toBe(false)
    expect(isBroadWatchRoot('/home/dev/repo', '/home/dev')).toBe(false)
    // A sibling that merely shares the home prefix as a string is not an ancestor.
    expect(isBroadWatchRoot('/home/devotee', '/home/dev')).toBe(false)
  })
})
