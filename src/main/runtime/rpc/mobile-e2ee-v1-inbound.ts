import { decrypt, decryptBytes } from './e2ee-crypto'
import type { MobileE2EEV1ReplayGuard } from './mobile-e2ee-v1-replay-guard'

// Why: mirrors handleDesktopMobileE2EEV2Inbound so the channel dispatches v1
// and v2 the same way. v1 framing is a random-nonce NaCl box with no sequence
// counter, so the replay guard (nonce-uniqueness) is the only in-band defense
// against a captured ciphertext being injected verbatim and re-dispatched.
export function handleMobileE2EEV1Inbound(args: {
  raw: string | Uint8Array<ArrayBufferLike>
  sharedKey: Uint8Array
  replayGuard: MobileE2EEV1ReplayGuard
  awaitingAuth: boolean
  ready: boolean
  onDecryptFailure: () => void
  onDecryptSuccess: () => void
  onAuth: (plaintext: string) => void
  onBinary: (plaintext: Uint8Array<ArrayBufferLike>) => void
  onText: (plaintext: string) => void
  onProtocolError: () => void
}): void {
  // A decryptable frame whose nonce we already accepted is a replay; drop it
  // (covers text + binary) and count it toward the failure cap.
  if (!args.replayGuard.accept(args.raw)) {
    args.onDecryptFailure()
    return
  }

  if (typeof args.raw !== 'string') {
    const plaintextBytes = decryptBytes(args.raw, args.sharedKey)
    if (plaintextBytes === null) {
      args.onDecryptFailure()
      return
    }
    args.onDecryptSuccess()
    if (!args.ready) {
      args.onProtocolError()
      return
    }
    args.onBinary(plaintextBytes)
    return
  }

  const plaintext = decrypt(args.raw, args.sharedKey)
  if (plaintext === null) {
    args.onDecryptFailure()
    return
  }
  args.onDecryptSuccess()
  if (args.awaitingAuth) {
    args.onAuth(plaintext)
    return
  }
  args.onText(plaintext)
}
