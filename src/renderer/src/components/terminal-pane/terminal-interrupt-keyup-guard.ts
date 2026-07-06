// Tracks the keyup half of a HOST-CLAIMED plain-Ctrl+C interrupt press so the
// paired 'c' release is suppressed exactly once (a claimed press already
// preventDefaulted; letting its keyup reach the engine could leak a kitty
// release report). The armed flag is cleared on terminal blur — capture phase,
// mirroring terminal-ime-native-text-forwarder — because a keyup that lands
// elsewhere after focus loss would otherwise leave the flag armed forever and
// a LATER unrelated 'c' keyup (even a plain one, typed after refocus) would be
// swallowed.

import { shouldSuppressTerminalInterruptKeyup, type XtermBypassEvent } from './xterm-bypass-policy'

export type TerminalInterruptKeyupGuard = {
  /** A host-claimed interrupt keydown happened; suppress its paired keyup. */
  arm: () => void
  /** The claim path saw the keyup itself (or aborted); drop the pending state. */
  disarm: () => void
  /** True when `event` is the armed claim's paired keyup — suppresses it and
   *  disarms. False (no side effects) for everything else. */
  claimKeyEvent: (event: XtermBypassEvent) => boolean
  dispose: () => void
}

export function createTerminalInterruptKeyupGuard(
  terminalElement: HTMLElement | null | undefined
): TerminalInterruptKeyupGuard {
  let armed = false
  const disarmOnBlur = (): void => {
    armed = false
  }
  terminalElement?.addEventListener('blur', disarmOnBlur, true)
  return {
    arm: () => {
      armed = true
    },
    disarm: () => {
      armed = false
    },
    claimKeyEvent: (event) => {
      if (!armed || !shouldSuppressTerminalInterruptKeyup(event)) {
        return false
      }
      armed = false
      return true
    },
    dispose: () => {
      armed = false
      terminalElement?.removeEventListener('blur', disarmOnBlur, true)
    }
  }
}
