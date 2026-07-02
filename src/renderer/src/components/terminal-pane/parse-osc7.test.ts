import { describe, it, expect } from 'vitest'
import { parseOsc7 } from './parse-osc7'
import { createAtermFacadeParser } from '../../lib/pane-manager/aterm/aterm-facade-parser'

describe('parseOsc7', () => {
  it('extracts a plain POSIX path', () => {
    expect(parseOsc7('file://host/home/jin/repo')).toBe('/home/jin/repo')
  })

  it('accepts an empty host', () => {
    expect(parseOsc7('file:///home/jin')).toBe('/home/jin')
  })

  it('percent-decodes spaces and unicode', () => {
    expect(parseOsc7('file:///home/jin/my%20code')).toBe('/home/jin/my code')
  })

  it('strips the leading slash before a Windows drive letter', () => {
    expect(parseOsc7('file:///C:/Users/jin/repo')).toBe('C:/Users/jin/repo')
  })

  it('preserves Windows UNC cwd paths', () => {
    expect(parseOsc7('file://server/share/project', { uncHost: 'server' })).toBe(
      '\\\\server\\share\\project'
    )
  })

  it('does not treat unrelated OSC-7 hosts as UNC servers', () => {
    expect(parseOsc7('file://remote/home/jin/repo', { uncHost: 'server' })).toBe('/home/jin/repo')
  })

  it('keeps POSIX host-prefixed paths unchanged by default', () => {
    expect(parseOsc7('file://server/share/project')).toBe('/share/project')
  })

  it('returns null for non-file URIs', () => {
    expect(parseOsc7('http://example.com/')).toBeNull()
  })

  it('returns null for unterminated/malformed input', () => {
    expect(parseOsc7('not a uri')).toBeNull()
    expect(parseOsc7('file://host')).toBeNull()
  })

  it('returns null for invalid percent-encoding', () => {
    expect(parseOsc7('file:///bad%ZZ')).toBeNull()
  })
})

// The engine pre-decodes OSC 7 to a path ('//host/path' for a named non-local
// host, bare '/path' for local); the facade re-encodes to the file:// wire form
// this parser consumes. The chain must preserve the host or UNC cwds break.
describe('parseOsc7 through the aterm facade re-encode', () => {
  function reencodeOsc7(payload: string): string {
    const { parser, dispatchOscEvent } = createAtermFacadeParser()
    let wire = ''
    parser.registerOscHandler(7, (data) => {
      wire = data
      return true
    })
    dispatchOscEvent(7, payload)
    return wire
  }

  it('preserves the host so a Windows UNC cwd survives the engine round trip', () => {
    const wire = reencodeOsc7('//server/share/my project')
    expect(wire).toBe('file://server/share/my%20project')
    expect(parseOsc7(wire, { uncHost: 'server' })).toBe('\\\\server\\share\\my project')
  })

  it('keeps non-UNC hosts POSIX through the round trip', () => {
    const wire = reencodeOsc7('//remote/home/jin/repo')
    expect(wire).toBe('file://remote/home/jin/repo')
    expect(parseOsc7(wire, { uncHost: 'server' })).toBe('/home/jin/repo')
  })

  it('re-encodes a bare (local) path as a host-less file URI', () => {
    const wire = reencodeOsc7('/home/jin/my code')
    expect(wire).toBe('file:///home/jin/my%20code')
    expect(parseOsc7(wire)).toBe('/home/jin/my code')
  })

  it('keeps a Windows drive-letter cwd working end to end', () => {
    const wire = reencodeOsc7('/C:/Users/jin/repo')
    expect(parseOsc7(wire)).toBe('C:/Users/jin/repo')
  })
})
