import { describe, expect, it } from 'vitest'
import {
  createOsc633CommandlineScanner,
  unescapeOsc633Commandline
} from './terminal-osc633-commandline'

const seq = (payload: string, terminator = '\x07'): string => `\x1b]633;E;${payload}${terminator}`

describe('createOsc633CommandlineScanner', () => {
  it('captures a BEL-terminated command line', () => {
    const scanner = createOsc633CommandlineScanner()
    scanner.scan(`prompt$ ${seq('npm run dev')}output`)
    expect(scanner.lastCommandline()).toBe('npm run dev')
  })

  it('captures an ST-terminated command line', () => {
    const scanner = createOsc633CommandlineScanner()
    scanner.scan(seq('ls -la', '\x1b\\'))
    expect(scanner.lastCommandline()).toBe('ls -la')
  })

  it('keeps the last of many sequences', () => {
    const scanner = createOsc633CommandlineScanner()
    scanner.scan(`${seq('first')}...${seq('second')}...${seq('third')}`)
    expect(scanner.lastCommandline()).toBe('third')
  })

  it('reassembles a sequence split across chunk boundaries', () => {
    const scanner = createOsc633CommandlineScanner()
    const full = seq('cargo build --release')
    // Every split point, including mid-prefix and mid-terminator.
    for (let split = 1; split < full.length; split += 1) {
      const chunked = createOsc633CommandlineScanner()
      chunked.scan(full.slice(0, split))
      expect(chunked.lastCommandline()).toBeNull()
      chunked.scan(full.slice(split))
      expect(chunked.lastCommandline()).toBe('cargo build --release')
    }
    scanner.scan(full)
    expect(scanner.lastCommandline()).toBe('cargo build --release')
  })

  it('reassembles an ST terminator split across chunks', () => {
    const scanner = createOsc633CommandlineScanner()
    scanner.scan('\x1b]633;E;make\x1b')
    expect(scanner.lastCommandline()).toBeNull()
    scanner.scan('\\')
    expect(scanner.lastCommandline()).toBe('make')
  })

  it('ignores a truncated tail with no terminator', () => {
    const scanner = createOsc633CommandlineScanner()
    scanner.scan(`${seq('kept')}garbage\x1b]633;E;never-termin`)
    expect(scanner.lastCommandline()).toBe('kept')
  })

  it('unescapes the VS Code payload escaping', () => {
    const scanner = createOsc633CommandlineScanner()
    scanner.scan(seq('echo \\x3bone\\x3b two\\x0athree \\\\ four'))
    expect(scanner.lastCommandline()).toBe('echo ;one; two\nthree \\ four')
  })

  it('takes only the command field when a nonce is appended un-escaped', () => {
    const scanner = createOsc633CommandlineScanner()
    scanner.scan(seq('git status;some-nonce'))
    expect(scanner.lastCommandline()).toBe('git status')
  })

  it('drops an oversized unterminated sequence but parses later ones', () => {
    const scanner = createOsc633CommandlineScanner()
    scanner.scan(`\x1b]633;E;${'x'.repeat(10000)}`)
    scanner.scan(`still-not-terminated`)
    expect(scanner.lastCommandline()).toBeNull()
    // Terminator for the oversized sequence arrives, then a fresh good one.
    scanner.scan(`\x07${seq('fresh')}`)
    expect(scanner.lastCommandline()).toBe('fresh')
  })

  it('ignores other 633 subcommands and OSC 133 sequences', () => {
    const scanner = createOsc633CommandlineScanner()
    scanner.scan('\x1b]633;A\x07\x1b]133;C\x07\x1b]633;P;Cwd=/home\x07')
    expect(scanner.lastCommandline()).toBeNull()
  })

  it('reset drops carry and remembered command', () => {
    const scanner = createOsc633CommandlineScanner()
    scanner.scan(seq('remembered'))
    scanner.scan('\x1b]633;E;partial')
    scanner.reset()
    expect(scanner.lastCommandline()).toBeNull()
    scanner.scan('rest\x07')
    expect(scanner.lastCommandline()).toBeNull()
  })
})

describe('unescapeOsc633Commandline', () => {
  it('round-trips the emission escaping', () => {
    expect(unescapeOsc633Commandline('a\\x3bb\\x0ac\\\\d')).toBe('a;b\nc\\d')
  })

  it('keeps unknown escapes and trailing backslashes verbatim', () => {
    expect(unescapeOsc633Commandline('tar -tf \\a')).toBe('tar -tf \\a')
    expect(unescapeOsc633Commandline('ends-with\\')).toBe('ends-with\\')
    // \x with non-hex suffix is not an escape.
    expect(unescapeOsc633Commandline('\\xzz')).toBe('\\xzz')
  })
})
