import { describe, expect, it } from 'vitest'
import { connect } from 'node:net'
import { spawn } from 'node:child_process'
import { existsSync, mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { StringDecoder } from 'node:string_decoder'
import { createNdjsonParser } from './ndjson'
import { createBinaryStreamParser } from './daemon-binary-stream-protocol'
import type { DaemonEvent } from './daemon-stream-events'

// End-to-end throughput + wire-parity gate for the v1020 binary stream plane.
// The REAL Rust daemon bench binary (rust/.../bin/stream-throughput-bench) plays
// the daemon's pump+encode over a Unix socket in each wire mode; this connects
// as the client and decodes with the REAL production parsers. So the measured
// delta is the true end-to-end cost — server encode + kernel socket + client
// parse — and both modes must deliver the SAME decoded PTY bytes (parity).
//
// Gated (bench binary needs the fork's Rust toolchain). Build + run with:
//   ~/.cargo/bin/cargo +trust build -p orca-daemon --bin stream-throughput-bench
//   ORCA_TERMINAL_PERF_BENCH=1 pnpm vitest run \
//     src/main/daemon/daemon-stream-frame-throughput.bench.test.ts \
//     --config config/vitest.config.ts
const benchEnabled = process.env.ORCA_TERMINAL_PERF_BENCH === '1'
const BENCH_BIN = ['debug', 'release']
  .map((p) => join(__dirname, '../../../rust/target', p, 'stream-throughput-bench'))
  .find(existsSync)

const TARGET_BYTES = 32 * 1024 * 1024
const CHUNK = 64 * 1024
const ROUNDS = 3

function controlHeavyTui(target: number): Buffer {
  const parts: Buffer[] = []
  let n = 0
  let i = 0
  while (n < target) {
    i++
    let s = `\x1b[${1 + (i % 40)};${1 + (i % 100)}H\x1b[38;5;${i % 256}m█░ cell ${i % 1000} \x1b[0m\r`
    if (i % 30 === 0) {
      s += '\x1b[2J\x1b[H'
    }
    const b = Buffer.from(s, 'utf8')
    parts.push(b)
    n += b.length
  }
  return Buffer.concat(parts)
}

type Mode = 'ndjson' | 'binary'
type Run = { ms: number; dataBytes: number; wireBytes: number }

function runOnce(
  bin: string,
  corpusPath: string,
  mode: Mode,
  dir: string,
  round: number
): Promise<Run> {
  return new Promise((resolve, reject) => {
    const sock = join(dir, `s-${mode}-${round}.sock`)
    const child = spawn(bin, [sock, corpusPath, mode, String(CHUNK)], {
      stdio: ['ignore', 'ignore', 'inherit']
    })
    child.on('error', reject)

    let dataBytes = 0
    let wireBytes = 0
    let started = 0n
    const onEvent = (e: DaemonEvent): void => {
      if (e.type === 'event' && e.event === 'data') {
        dataBytes += Buffer.byteLength(e.payload.data, 'utf8')
      }
    }
    const binaryParser = createBinaryStreamParser(onEvent)
    const decoder = new StringDecoder('utf8')
    const ndjsonParser = createNdjsonParser((m) => onEvent(m as DaemonEvent))

    const attempt = (tries: number): void => {
      const socket = connect(sock)
      socket.on('connect', () => {
        socket.on('data', (chunk: Buffer) => {
          if (started === 0n) {
            started = process.hrtime.bigint()
          }
          wireBytes += chunk.length
          if (mode === 'binary') {
            binaryParser.feed(chunk)
          } else {
            ndjsonParser.feed(decoder.write(chunk))
          }
        })
        socket.on('end', () =>
          resolve({ ms: Number(process.hrtime.bigint() - started) / 1e6, dataBytes, wireBytes })
        )
      })
      // The server binds then accepts; retry to lose the connect/bind race.
      socket.on('error', (err) =>
        tries > 0 ? setTimeout(() => attempt(tries - 1), 25) : reject(err)
      )
    }
    setTimeout(() => attempt(40), 40)
  })
}

const median = (xs: number[]): number => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)]

describe.skipIf(!benchEnabled || !BENCH_BIN)(
  'daemon stream-frame throughput (binary vs NDJSON)',
  () => {
    it('binary frames are faster, smaller on the wire, and byte-identical after decode', async () => {
      const bin = BENCH_BIN as string
      const dir = mkdtempSync(join(tmpdir(), 'stream-frame-'))
      const corpusPath = join(dir, 'control-heavy-tui.bin')
      const corpus = controlHeavyTui(TARGET_BYTES)
      writeFileSync(corpusPath, corpus)

      const rates: Record<Mode, number[]> = { ndjson: [], binary: [] }
      const last: Record<Mode, Run> = {
        ndjson: { ms: 0, dataBytes: 0, wireBytes: 0 },
        binary: { ms: 0, dataBytes: 0, wireBytes: 0 }
      }
      for (const mode of ['ndjson', 'binary'] as Mode[]) {
        for (let r = 0; r < ROUNDS; r++) {
          const run = await runOnce(bin, corpusPath, mode, dir, r)
          rates[mode].push(corpus.length / 1024 / 1024 / (run.ms / 1000))
          last[mode] = run
        }
      }
      const nd = median(rates.ndjson)
      const bi = median(rates.binary)

      // eslint-disable-next-line no-console -- bench harness output
      console.log(
        `\n[stream-frame] control-heavy-tui ${(corpus.length / 1024 / 1024).toFixed(0)}MB\n` +
          `  NDJSON ${nd.toFixed(1)} MB/s wire ${(last.ndjson.wireBytes / 1e6).toFixed(1)}MB\n` +
          `  binary ${bi.toFixed(1)} MB/s wire ${(last.binary.wireBytes / 1e6).toFixed(1)}MB\n` +
          `  speedup ${(bi / nd).toFixed(2)}x · wire -${((1 - last.binary.wireBytes / last.ndjson.wireBytes) * 100).toFixed(1)}%`
      )

      // Parity: identical decoded PTY bytes both ways (deterministic).
      expect(last.binary.dataBytes).toBe(last.ndjson.dataBytes)
      // Wire reduction is deterministic: NDJSON escapes every control byte to
      // \uXXXX (6 bytes), binary sends them raw.
      expect(last.binary.wireBytes).toBeLessThan(last.ndjson.wireBytes)
      // Throughput win (loose bound to tolerate loaded CI; the real margin is large).
      expect(bi).toBeGreaterThan(nd)
    }, 120_000)
  }
)
