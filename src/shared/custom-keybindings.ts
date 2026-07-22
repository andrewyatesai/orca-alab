import {
  getKeybindingDefinition,
  isKeybindingActionId,
  keybindingMatchesInput,
  type CustomKeybindingActionId,
  type KeybindingInput
} from './keybindings'

export type { CustomKeybindingActionId } from './keybindings'

export type CustomKeybindingActionSpec =
  | { type: 'sendText'; text: string }
  | { type: 'runQuickCommand'; quickCommandId: string }

export type CustomKeybinding = {
  id: CustomKeybindingActionId
  title: string
  action: CustomKeybindingActionSpec
  /** Canonical chord strings (same grammar as built-in overrides). */
  bindings: string[]
  /** Match the chord's key token against event.code even when an IME rewrote
   *  event.key to a composed char (full-width 。，). Default false. */
  matchPhysicalKey?: boolean
  /** RESERVED — parsed and preserved but not evaluated in v1 (see design §8). */
  when?: { hostPlatform?: 'darwin' | 'linux' | 'win32'; connection?: 'local' | 'ssh' | 'wsl' }
}

/** Parse-time enrichment; this is what the snapshot ships to the renderer. */
export type ResolvedCustomKeybinding = CustomKeybinding & {
  /** Present iff action.type === 'sendText' and escapes decoded cleanly. */
  decodedText?: string
}

export const CUSTOM_KEYBINDING_ID_PATTERN = /^custom\.[a-z0-9]{4,32}$/

/** Paste-bomb guard: large payloads belong in Quick Commands, which chunk via the input write queue. */
export const CUSTOM_SEND_TEXT_MAX_BYTES = 4096

export const CUSTOM_KEYBINDING_TITLE_MAX_LENGTH = 64

const CUSTOM_ID_ALPHABET = 'abcdefghijklmnopqrstuvwxyz0123456789'

export function generateCustomKeybindingId(): CustomKeybindingActionId {
  let suffix = ''
  for (let index = 0; index < 12; index++) {
    suffix += CUSTOM_ID_ALPHABET[Math.floor(Math.random() * CUSTOM_ID_ALPHABET.length)]
  }
  return `custom.${suffix}`
}

export type CustomSendTextDecodeResult =
  | { ok: true; text: string }
  | { ok: false; error: string }

const HEX_PATTERN = /^[0-9a-fA-F]+$/

/**
 * Decodes the user-typed escape grammar for sendText payloads. Unknown escapes
 * are errors (not pass-through — silent pass-through is how iTerm2 configs rot).
 */
export function decodeCustomSendText(raw: string): CustomSendTextDecodeResult {
  let text = ''
  for (let index = 0; index < raw.length; index++) {
    const char = raw[index]
    if (char !== '\\') {
      text += char
      continue
    }
    const next = raw[index + 1]
    if (next === undefined) {
      return { ok: false, error: 'Trailing backslash — use \\\\ for a literal backslash.' }
    }
    switch (next) {
      case 'e':
        text += '\x1b'
        index += 1
        break
      case 'n':
        text += '\n'
        index += 1
        break
      case 'r':
        text += '\r'
        index += 1
        break
      case 't':
        text += '\t'
        index += 1
        break
      case '0':
        text += '\0'
        index += 1
        break
      case '\\':
        text += '\\'
        index += 1
        break
      case 'x': {
        const hex = raw.slice(index + 2, index + 4)
        if (hex.length !== 2 || !HEX_PATTERN.test(hex)) {
          return { ok: false, error: 'Use two hex digits after \\x, like \\x1b.' }
        }
        text += String.fromCharCode(Number.parseInt(hex, 16))
        index += 3
        break
      }
      case 'u': {
        if (raw[index + 2] === '{') {
          const closeIndex = raw.indexOf('}', index + 3)
          const hex = closeIndex === -1 ? '' : raw.slice(index + 3, closeIndex)
          if (closeIndex === -1 || hex.length === 0 || hex.length > 6 || !HEX_PATTERN.test(hex)) {
            return { ok: false, error: 'Use \\u{...} with 1–6 hex digits, like \\u{1F600}.' }
          }
          const codePoint = Number.parseInt(hex, 16)
          if (codePoint > 0x10ffff) {
            return { ok: false, error: '\\u{...} code point must be at most 0x10FFFF.' }
          }
          text += String.fromCodePoint(codePoint)
          index = closeIndex
          break
        }
        const hex = raw.slice(index + 2, index + 6)
        if (hex.length !== 4 || !HEX_PATTERN.test(hex)) {
          return { ok: false, error: 'Use four hex digits after \\u, like \\u001b.' }
        }
        text += String.fromCharCode(Number.parseInt(hex, 16))
        index += 5
        break
      }
      default:
        return { ok: false, error: `Unknown escape "\\${next}". Use \\\\ for a literal backslash.` }
    }
  }
  if (text === '') {
    return { ok: false, error: 'Text to send must not be empty.' }
  }
  return { ok: true, text }
}

export type CustomKeybindingMatch = {
  entry: ResolvedCustomKeybinding
  /** The specific chord that matched (an entry can carry several bindings). */
  binding: string
}

/**
 * First entry wins; Settings-side conflict detection makes ordering ambiguity
 * unrepresentable for saved configs. All layout/AltGr/Option guards come from
 * the shared matcher.
 */
export function matchCustomKeybinding(
  entries: readonly ResolvedCustomKeybinding[],
  input: KeybindingInput,
  platform: NodeJS.Platform
): CustomKeybindingMatch | null {
  for (const entry of entries) {
    for (const binding of entry.bindings) {
      if (keybindingMatchesInput(binding, input, platform)) {
        return { entry, binding }
      }
      // Why: an empty key flips the shared matcher into its physical-code path,
      // so a bare Period chord fires when a CJK IME reports key '。' code 'Period' (#9338).
      if (
        entry.matchPhysicalKey === true &&
        keybindingMatchesInput(binding, { ...input, key: '' }, platform)
      ) {
        return { entry, binding }
      }
    }
  }
  return null
}

/** Title for a built-in or custom action id, falling back to the raw id. */
export function resolveKeybindingTitle(
  id: string,
  customEntries?: readonly CustomKeybinding[]
): string {
  if (isKeybindingActionId(id)) {
    return getKeybindingDefinition(id)?.title ?? id
  }
  return customEntries?.find((entry) => entry.id === id)?.title ?? id
}

export function isCustomKeybindingId(value: string): value is CustomKeybindingActionId {
  return CUSTOM_KEYBINDING_ID_PATTERN.test(value)
}
