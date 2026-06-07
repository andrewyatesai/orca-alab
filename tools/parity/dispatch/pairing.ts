// TS dispatch for the pairing parity module: maps the shared vector function
// names to the real `src/shared/pairing.ts` exports so the harness compares the
// live TS reference against the Rust port.

import {
  decodePairingOffer,
  encodePairingOffer,
  parsePairingCode,
  type PairingOffer
} from '../../../src/shared/pairing'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'encodePairingOffer':
      return encodePairingOffer(input as PairingOffer)
    case 'decodePairingOffer':
      // decodePairingOffer throws on a bad URL/payload; normalize the throw to
      // null so the Rust `Result::Err` arm has the same JSON image to compare.
      try {
        return decodePairingOffer(input as string)
      } catch {
        return null
      }
    case 'parsePairingCode':
      return parsePairingCode(input as string)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
