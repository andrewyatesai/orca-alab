// TS dispatch for the uri-component parity module. There is no dedicated
// src/shared source file: encode/decode mirror the JS globals
// encodeURIComponent / decodeURIComponent. The Rust port returns the input
// unchanged when decodeURIComponent would throw on a malformed %-escape, so the
// reference decode wraps the global in the same try/catch to stay JSON-equal.

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'encodeURIComponent':
      return encodeURIComponent(input as string)
    case 'decodeURIComponent': {
      const s = input as string
      try {
        return decodeURIComponent(s)
      } catch {
        return s
      }
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
