// TS dispatch for the terminal-stream-protocol parity module: maps the shared
// vector function names to the real `src/shared/terminal-stream-protocol.ts`
// exports so the harness compares the live TS reference against the Rust port.
//
// Byte buffers (Uint8Array / Vec<u8>) cross the vector boundary as plain number
// arrays so goldens stay valid JSON and compare structurally against the Rust
// side; this adapter converts at the edge and calls the real functions.

import {
  decodeTerminalStreamFrame,
  decodeTerminalStreamJson,
  decodeTerminalStreamText,
  encodeTerminalStreamFrame,
  encodeTerminalStreamJson,
  encodeTerminalStreamText,
  type TerminalStreamFrame
} from '../../../src/shared/terminal-stream-protocol'

function toBytes(value: number[]): Uint8Array {
  return Uint8Array.from(value)
}

function fromBytes(bytes: Uint8Array): number[] {
  return Array.from(bytes)
}

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'encodeTerminalStreamFrame': {
      const { opcode, streamId, seq, payload } = input as {
        opcode: number
        streamId: number
        seq: number
        payload: number[]
      }
      const frame = { opcode, streamId, seq, payload: toBytes(payload) } as TerminalStreamFrame
      return fromBytes(encodeTerminalStreamFrame(frame))
    }
    case 'decodeTerminalStreamFrame': {
      const { bytes } = input as { bytes: number[] }
      const decoded = decodeTerminalStreamFrame(toBytes(bytes))
      if (!decoded) return null
      return {
        opcode: decoded.opcode,
        streamId: decoded.streamId,
        seq: decoded.seq,
        payload: fromBytes(decoded.payload)
      }
    }
    case 'encodeTerminalStreamJson': {
      const { value } = input as { value: unknown }
      return fromBytes(encodeTerminalStreamJson(value))
    }
    case 'decodeTerminalStreamJson': {
      const { payload } = input as { payload: number[] }
      return decodeTerminalStreamJson(toBytes(payload))
    }
    case 'encodeTerminalStreamText': {
      const { value } = input as { value: string }
      return fromBytes(encodeTerminalStreamText(value))
    }
    case 'decodeTerminalStreamText': {
      const { payload } = input as { payload: number[] }
      return decodeTerminalStreamText(toBytes(payload))
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
