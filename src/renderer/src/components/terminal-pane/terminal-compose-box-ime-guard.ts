import { TERMINAL_IME_ENTER_REDISPATCH_ABSORB_WINDOW_MS } from './terminal-ime-deferred-newline'

export { TERMINAL_IME_ENTER_REDISPATCH_ABSORB_WINDOW_MS }

export type ComposeBoxImeEnterGuard = {
  onCompositionStart(): void
  /** Arms one absorb credit with a deadline for the re-dispatched commit Enter. */
  onCompositionEnd(): void
  /** True → swallow this Enter (composing, keyCode 229, or re-dispatch shortly after compositionend). */
  shouldAbsorbEnter(event: { isComposing: boolean; keyCode: number }): boolean
}

/**
 * Enter-vs-IME guard for the compose box textarea. The plain composing check
 * (isComposing / keyCode 229) is not sufficient on macOS Hangul: the committing
 * Enter is re-dispatched as a plain keydown (isComposing=false) ~2 ms after
 * compositionend, which would submit the draft. One absorb credit armed at
 * compositionend swallows exactly that re-dispatch; the credit expires so it
 * can never eat a later real Enter.
 */
export function createComposeBoxImeEnterGuard(
  now: () => number = Date.now
): ComposeBoxImeEnterGuard {
  let composing = false
  let absorbCredit = false
  let absorbDeadline = 0

  return {
    onCompositionStart() {
      composing = true
    },
    onCompositionEnd() {
      composing = false
      absorbCredit = true
      absorbDeadline = now() + TERMINAL_IME_ENTER_REDISPATCH_ABSORB_WINDOW_MS
    },
    shouldAbsorbEnter(event) {
      if (composing || event.isComposing || event.keyCode === 229) {
        return true
      }
      if (absorbCredit && now() <= absorbDeadline) {
        absorbCredit = false
        return true
      }
      absorbCredit = false
      return false
    }
  }
}
