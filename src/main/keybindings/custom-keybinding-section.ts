import {
  CUSTOM_KEYBINDING_ID_PATTERN,
  CUSTOM_KEYBINDING_TITLE_MAX_LENGTH,
  CUSTOM_SEND_TEXT_MAX_BYTES,
  decodeCustomSendText,
  type CustomKeybinding,
  type CustomKeybindingActionSpec,
  type ResolvedCustomKeybinding
} from '../../shared/custom-keybindings'
import {
  keybindingChordHasNoNonShiftModifiers,
  normalizeCustomKeybindingChord,
  type KeybindingFileDiagnostic
} from '../../shared/keybindings'

type JsonObject = Record<string, unknown>

const KNOWN_ENTRY_KEYS = new Set(['id', 'title', 'action', 'bindings', 'matchPhysicalKey', 'when'])

function isJsonObject(value: unknown): value is JsonObject {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value)
}

type CustomEntrySanitizeResult =
  | { ok: true; entry: ResolvedCustomKeybinding; warnings: string[] }
  | { ok: false; error: string }

function sanitizeAction(value: unknown): { ok: true; action: CustomKeybindingActionSpec; decodedText?: string } | { ok: false; error: string } {
  if (!isJsonObject(value)) {
    return { ok: false, error: 'action must be an object.' }
  }
  if (value.type === 'sendText') {
    if (typeof value.text !== 'string') {
      return { ok: false, error: 'action.text must be a string.' }
    }
    const decoded = decodeCustomSendText(value.text)
    if (!decoded.ok) {
      return { ok: false, error: `action.text: ${decoded.error}` }
    }
    // Why: paste-bomb guard — large payloads belong in Quick Commands, which chunk via the input write queue.
    if (new TextEncoder().encode(decoded.text).length > CUSTOM_SEND_TEXT_MAX_BYTES) {
      return {
        ok: false,
        error: `action.text decodes to more than ${CUSTOM_SEND_TEXT_MAX_BYTES} bytes.`
      }
    }
    return { ok: true, action: { type: 'sendText', text: value.text }, decodedText: decoded.text }
  }
  if (value.type === 'runQuickCommand') {
    if (typeof value.quickCommandId !== 'string' || value.quickCommandId.length === 0) {
      return { ok: false, error: 'action.quickCommandId must be a non-empty string.' }
    }
    return { ok: true, action: { type: 'runQuickCommand', quickCommandId: value.quickCommandId } }
  }
  return { ok: false, error: 'action.type must be "sendText" or "runQuickCommand".' }
}

function sanitizeCustomEntry(raw: unknown): CustomEntrySanitizeResult {
  if (!isJsonObject(raw)) {
    return { ok: false, error: 'entry must be an object.' }
  }
  const warnings: string[] = []
  if (typeof raw.id !== 'string' || !CUSTOM_KEYBINDING_ID_PATTERN.test(raw.id)) {
    return { ok: false, error: 'id must match custom.<4-32 lowercase letters/digits>.' }
  }
  const id = raw.id as ResolvedCustomKeybinding['id']
  if (typeof raw.title !== 'string' || raw.title.trim().length === 0) {
    return { ok: false, error: 'title must be a non-empty string.' }
  }
  let title = raw.title.trim()
  if (title.length > CUSTOM_KEYBINDING_TITLE_MAX_LENGTH) {
    title = title.slice(0, CUSTOM_KEYBINDING_TITLE_MAX_LENGTH)
    warnings.push(`title was truncated to ${CUSTOM_KEYBINDING_TITLE_MAX_LENGTH} characters.`)
  }
  const action = sanitizeAction(raw.action)
  if (!action.ok) {
    return { ok: false, error: action.error }
  }
  if (!Array.isArray(raw.bindings) || !raw.bindings.every((item) => typeof item === 'string')) {
    return { ok: false, error: 'bindings must be an array of shortcut strings.' }
  }
  const bindings: string[] = []
  for (const binding of raw.bindings) {
    const normalized = normalizeCustomKeybindingChord(binding)
    if (!normalized.ok) {
      return { ok: false, error: `binding "${binding}": ${normalized.error}` }
    }
    if (!bindings.includes(normalized.value)) {
      bindings.push(normalized.value)
    }
  }
  let matchPhysicalKey: boolean | undefined
  if (raw.matchPhysicalKey !== undefined) {
    if (typeof raw.matchPhysicalKey === 'boolean') {
      matchPhysicalKey = raw.matchPhysicalKey
    } else {
      warnings.push('matchPhysicalKey must be a boolean and was ignored.')
    }
  }
  // Why: `when` is reserved for v2 scoping — parse and carry it so hand-authored clauses round-trip unevaluated.
  const when = isJsonObject(raw.when) ? (raw.when as CustomKeybinding['when']) : undefined
  for (const key of Object.keys(raw)) {
    if (!KNOWN_ENTRY_KEYS.has(key)) {
      warnings.push(`unknown key "${key}" is preserved but ignored.`)
    }
  }
  for (const binding of bindings) {
    if (keybindingChordHasNoNonShiftModifiers(binding)) {
      warnings.push(
        `"${binding}" has no modifier, so it will no longer type its character in terminals.`
      )
    }
  }
  const entry: ResolvedCustomKeybinding = {
    id,
    title,
    action: action.action,
    bindings,
    ...(matchPhysicalKey !== undefined ? { matchPhysicalKey } : {}),
    ...(when !== undefined ? { when } : {}),
    ...(action.decodedText !== undefined ? { decodedText: action.decodedText } : {})
  }
  return { ok: true, entry, warnings }
}

export function parseCustomKeybindingSection(
  value: unknown,
  diagnostics: KeybindingFileDiagnostic[]
): ResolvedCustomKeybinding[] {
  if (value === undefined) {
    return []
  }
  if (!Array.isArray(value)) {
    diagnostics.push({
      severity: 'error',
      section: 'custom',
      message: 'custom must be an array of custom shortcut entries.'
    })
    return []
  }
  const entries: ResolvedCustomKeybinding[] = []
  const seenIds = new Set<string>()
  value.forEach((raw, index) => {
    const label =
      isJsonObject(raw) && typeof raw.id === 'string' ? raw.id : `custom[${String(index)}]`
    const result = sanitizeCustomEntry(raw)
    if (!result.ok) {
      diagnostics.push({
        severity: 'error',
        section: 'custom',
        actionId: label,
        message: `Custom shortcut "${label}" was ignored: ${result.error}`
      })
      return
    }
    if (seenIds.has(result.entry.id)) {
      diagnostics.push({
        severity: 'error',
        section: 'custom',
        actionId: result.entry.id,
        message: `Custom shortcut "${result.entry.id}" was ignored: duplicate id.`
      })
      return
    }
    seenIds.add(result.entry.id)
    for (const warning of result.warnings) {
      diagnostics.push({
        severity: 'warning',
        section: 'custom',
        actionId: result.entry.id,
        message: `Custom shortcut "${result.entry.title}": ${warning}`
      })
    }
    entries.push(result.entry)
  })
  return entries
}

/** Write-path validation: same predicates as the parser, but throwing (mirrors writeKeybindingOverride). */
export function validateCustomKeybindingForWrite(entry: CustomKeybinding): ResolvedCustomKeybinding {
  const result = sanitizeCustomEntry(entry as unknown)
  if (!result.ok) {
    throw new Error(result.error)
  }
  return result.entry
}

/**
 * On-disk shape for an upserted entry: canonical bindings, raw (escaped)
 * action text as typed, and every unrecognized key of the previous stored
 * entry preserved (downgrade symmetry — see design §4).
 */
export function serializeCustomKeybindingEntry(
  entry: ResolvedCustomKeybinding,
  previousRaw: unknown
): JsonObject {
  const preserved: JsonObject = isJsonObject(previousRaw) ? { ...previousRaw } : {}
  delete preserved.matchPhysicalKey
  const serialized: JsonObject = {
    ...preserved,
    id: entry.id,
    title: entry.title,
    action: entry.action,
    bindings: [...entry.bindings]
  }
  if (entry.matchPhysicalKey !== undefined) {
    serialized.matchPhysicalKey = entry.matchPhysicalKey
  }
  if (entry.when !== undefined) {
    serialized.when = entry.when
  }
  return serialized
}
