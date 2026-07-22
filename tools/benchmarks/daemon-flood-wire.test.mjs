import { describe, expect, it } from 'vitest'
import {
  DAEMON_PROTOCOL_VERSION,
  createOrAttachLine,
  helloLine,
  makeExitEventScanner,
  sshForwardArgs,
  sshPreflightArgs,
  summarizeRates
} from './daemon-flood-wire.mjs'
import { parseFloodArgs } from './daemon-flood-timed.mjs'

describe('helloLine', () => {
  it('speaks the v1020 hello shape, token-less', () => {
    const obj = JSON.parse(helloLine('control'))
    expect(obj).toEqual({
      type: 'hello',
      version: DAEMON_PROTOCOL_VERSION,
      token: '',
      clientId: 'flood',
      role: 'control'
    })
    expect(helloLine('control').endsWith('\n')).toBe(true)
  })

  it('negotiates the binary stream format only on the stream role', () => {
    expect(JSON.parse(helloLine('stream', { binaryStream: true })).streamFormat).toBe('binary')
    expect(JSON.parse(helloLine('stream')).streamFormat).toBeUndefined()
    expect(JSON.parse(helloLine('control', { binaryStream: true })).streamFormat).toBeUndefined()
  })
})

describe('createOrAttachLine', () => {
  it('floods via sh -c with the corpus as $1 so spaced paths survive', () => {
    const obj = JSON.parse(
      createOrAttachLine({
        id: 'c1',
        sessionId: 's',
        corpusPath: '/tmp/with space/corpus.vt',
        platform: 'darwin'
      })
    )
    expect(obj.payload.shellOverride).toBe('/bin/sh')
    expect(obj.payload.shellArgs).toEqual(['-c', 'cat "$1"', 'flood', '/tmp/with space/corpus.vt'])
    expect(obj.payload.cols).toBe(120)
    expect(obj.payload.rows).toBe(40)
  })

  it('uses cmd.exe type on win32', () => {
    const obj = JSON.parse(
      createOrAttachLine({ id: 'c1', sessionId: 's', corpusPath: 'C:\\t\\c.vt', platform: 'win32' })
    )
    expect(obj.payload.shellOverride).toBe('cmd.exe')
    expect(obj.payload.shellArgs).toEqual(['/d', '/s', '/c', 'type "C:\\t\\c.vt"'])
  })
})

describe('makeExitEventScanner', () => {
  const exitEvent = Buffer.from(
    '{"type":"event","event":"exit","sessionId":"s","payload":{"code":0}}\n'
  )

  it('sees the exit event inside a single chunk', () => {
    const s = makeExitEventScanner()
    expect(s.push(Buffer.from('data data data'))).toBe(false)
    expect(s.push(Buffer.concat([Buffer.from('tail-of-flood'), exitEvent]))).toBe(true)
    expect(s.sawExit).toBe(true)
  })

  it('sees an exit marker that straddles a chunk boundary', () => {
    const s = makeExitEventScanner()
    // Split mid-needle: `"event":"ex` ‖ `it"` — only the tail overlap can catch it.
    const cut = exitEvent.indexOf('exit"') + 2
    expect(s.push(exitEvent.subarray(0, cut))).toBe(false)
    expect(s.push(exitEvent.subarray(cut))).toBe(true)
  })

  it('detects the marker inside a v1020 binary Event frame', () => {
    // protocol.rs::event_frame — [0x07][len u32 BE][json]; JSON text is verbatim.
    const json = exitEvent.subarray(0, -1)
    const header = Buffer.alloc(5)
    header[0] = 0x07
    header.writeUInt32BE(json.length, 1)
    const s = makeExitEventScanner()
    expect(s.push(Buffer.concat([header, json]))).toBe(true)
  })

  it('counts wire bytes up to and including the marker chunk, then stops', () => {
    const s = makeExitEventScanner()
    s.push(Buffer.from('12345'))
    s.push(exitEvent)
    const atExit = 5 + exitEvent.length
    expect(s.wireBytes).toBe(atExit)
    expect(s.push(Buffer.from('straggler'))).toBe(true)
    expect(s.wireBytes).toBe(atExit)
  })

  it('does not fire on ordinary data events', () => {
    const s = makeExitEventScanner()
    const data = '{"type":"event","event":"data","sessionId":"s","payload":{"data":"x"}}\n'
    expect(s.push(Buffer.from(data.repeat(50)))).toBe(false)
  })
})

describe('ssh argv builders', () => {
  it('forwards local→remote Unix sockets with idempotent-rebind options', () => {
    const args = sshForwardArgs({
      destination: 'localhost',
      localSocket: '/tmp/l.sock',
      remoteSocket: '/tmp/r.sock',
      extraSshArgs: ['-p', '2222']
    })
    expect(args[0]).toBe('-N')
    expect(args).toContain('StreamLocalBindUnlink=yes')
    expect(args).toContain('ExitOnForwardFailure=yes')
    const l = args.indexOf('-L')
    expect(args[l + 1]).toBe('/tmp/l.sock:/tmp/r.sock')
    expect(args.at(-1)).toBe('localhost')
    expect(args.join(' ')).toContain('-p 2222')
  })

  it('preflight runs `true` non-interactively at the destination', () => {
    const args = sshPreflightArgs({ destination: 'localhost', extraSshArgs: ['-i', '/k'] })
    expect(args.slice(-2)).toEqual(['localhost', 'true'])
    expect(args).toContain('BatchMode=yes')
    expect(args.join(' ')).toContain('-i /k')
  })
})

describe('summarizeRates', () => {
  it('reports median/mean/min/max (odd and even counts)', () => {
    expect(summarizeRates([3, 1, 2])).toEqual({ median: 2, mean: 2, min: 1, max: 3 })
    expect(summarizeRates([4, 1, 2, 3]).median).toBe(2.5)
  })

  it('rejects an empty series', () => {
    expect(() => summarizeRates([])).toThrow('no values')
  })
})

describe('parseFloodArgs', () => {
  it('defaults to native ndjson, 200 MB, 5 trials', () => {
    expect(parseFloodArgs([])).toMatchObject({
      mode: 'native',
      binary: false,
      mb: 200,
      trials: 5,
      sshDest: 'localhost',
      sshArgs: []
    })
  })

  it('parses the full flag set, accumulating --ssh-arg', () => {
    const opts = parseFloodArgs([
      '--mode',
      'ssh-localhost',
      '--binary',
      '--mb',
      '500',
      '--trials',
      '3',
      '--daemon-bin',
      '/bin/d',
      '--ssh-dest',
      '127.0.0.1',
      '--ssh-arg',
      '-p',
      '--ssh-arg',
      '2222',
      '--label',
      'baseline'
    ])
    expect(opts).toMatchObject({
      mode: 'ssh-localhost',
      binary: true,
      mb: 500,
      trials: 3,
      daemonBin: '/bin/d',
      sshDest: '127.0.0.1',
      sshArgs: ['-p', '2222'],
      label: 'baseline'
    })
  })

  it('rejects unknown modes, unknown flags, and non-positive numbers', () => {
    expect(() => parseFloodArgs(['--mode', 'wsl'])).toThrow('--mode must be')
    expect(() => parseFloodArgs(['--frobnicate'])).toThrow('unknown arg')
    expect(() => parseFloodArgs(['--mb', '-5'])).toThrow('positive')
    expect(() => parseFloodArgs(['--trials'])).toThrow('requires a value')
  })
})
