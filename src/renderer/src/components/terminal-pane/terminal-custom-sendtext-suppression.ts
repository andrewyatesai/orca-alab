import {
  createTerminalNativeOnlyShortcutTracker,
  type TerminalNativeOnlyShortcutTracker
} from './terminal-native-only-shortcut'

export type TerminalCustomSendTextSuppression = TerminalNativeOnlyShortcutTracker

/**
 * Companion suppression for custom sendText chords with no non-Shift modifiers
 * (bare/Shift-only printable remaps). The arming contract is the INVERSE of the
 * native-only tracker's: here the keydown default IS canceled (the payload
 * replaces typing), and this tracker swallows what a canceled keydown does not
 * stop — IME direct-inserts arriving via `beforeinput` (primary path in modern
 * Chromium) plus legacy `keypress` best-effort — and the keyup, so the armed
 * key never leaks into the engine. A separate instance keeps the two arming
 * contracts from entangling; only the event-pairing mechanics are shared.
 */
export function createTerminalCustomSendTextSuppression(): TerminalCustomSendTextSuppression {
  return createTerminalNativeOnlyShortcutTracker()
}
