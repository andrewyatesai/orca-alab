// TS dispatch for the browser-screencast-protocol parity module: maps the
// shared vector function names to the real
// `src/shared/browser-screencast-protocol.ts` exports. Byte buffers cross the
// vector boundary as plain number arrays so the goldens stay valid JSON.

import {
  BrowserScreencastOpcode,
  decodeBrowserScreencastFrame,
  encodeBrowserScreencastFrame,
  type BrowserScreencastFormat,
  type BrowserScreencastFrameMetadata
} from '../../../src/shared/browser-screencast-protocol'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'encodeBrowserScreencastFrame': {
      const { seq, format, metadata, image } = input as {
        seq: number
        format: BrowserScreencastFormat
        metadata: BrowserScreencastFrameMetadata
        image: number[]
      }
      return Array.from(
        encodeBrowserScreencastFrame({
          opcode: BrowserScreencastOpcode.Frame,
          seq,
          format,
          metadata,
          image: new Uint8Array(image)
        })
      )
    }
    case 'decodeBrowserScreencastFrame': {
      const { bytes } = input as { bytes: number[] }
      const decoded = decodeBrowserScreencastFrame(new Uint8Array(bytes))
      if (!decoded) {
        return null
      }
      return {
        opcode: decoded.opcode,
        seq: decoded.seq,
        format: decoded.format,
        metadata: decoded.metadata,
        image: Array.from(decoded.image)
      }
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
