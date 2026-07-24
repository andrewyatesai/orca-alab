// Why: the legacy v1 E2EE frame is a random-nonce NaCl box with no sequence
// counter (unlike v2, which enforces a monotonic per-direction counter). A
// verbatim replayed ciphertext therefore decrypts cleanly and would re-run the
// method. Genuine frames each carry a fresh 24-byte random nonce, so
// nonce-uniqueness is the only in-band replay guard available without a
// wire-format or client change. This closes the v1 replay downgrade for the
// direct-transport clients (web, remote-runtime) that still negotiate v1.
const NONCE_LENGTH = 24
// Bounds worst-case memory (~1.2MB) so a peer cannot grow the set unbounded.
// A live on-path replay is injected near-immediately, so a recent-nonce window
// covers the realistic attack; delayed replays past the window are the residual
// tradeoff of keeping v1 wire-compatible (v2 counters are the complete fix).
const DEFAULT_MAX_TRACKED_NONCES = 8192

export class MobileE2EEV1ReplayGuard {
  private readonly seen = new Set<string>()
  private readonly max: number

  constructor(maxTrackedNonces: number = DEFAULT_MAX_TRACKED_NONCES) {
    this.max = maxTrackedNonces
  }

  // Returns true when the frame's nonce is new (accept it); false when the
  // nonce was already seen (replay) or the frame is too short to carry one.
  accept(frame: string | Uint8Array<ArrayBufferLike>): boolean {
    const nonceB64 = this.nonceOf(frame)
    if (nonceB64 === null || this.seen.has(nonceB64)) {
      return false
    }
    if (this.seen.size >= this.max) {
      // Insertion-ordered Set: the first key is the oldest tracked nonce.
      const oldest = this.seen.values().next().value
      if (oldest !== undefined) {
        this.seen.delete(oldest)
      }
    }
    this.seen.add(nonceB64)
    return true
  }

  clear(): void {
    this.seen.clear()
  }

  private nonceOf(frame: string | Uint8Array<ArrayBufferLike>): string | null {
    const bytes = typeof frame === 'string' ? Buffer.from(frame, 'base64') : Buffer.from(frame)
    if (bytes.length < NONCE_LENGTH) {
      return null
    }
    return bytes.subarray(0, NONCE_LENGTH).toString('base64')
  }
}
