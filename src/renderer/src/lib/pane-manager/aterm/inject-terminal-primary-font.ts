// Honor the user's terminalFontFamily: resolve it to its primary-face bytes on the
// host (window.api.fonts.resolvePrimaryFont) and inject it as the engine's primary
// face (set_primary_font). The bundled JetBrains Mono is the engine's built-in
// primary, so a JetBrains-Mono (or unset) family needs no injection. Graceful: any
// failure (unresolvable family, unloadable blob) keeps the bundled face — never throws.

type PrimaryFontInjectable = { set_primary_font: (bytes: Uint8Array) => void }

const BUNDLED_PRIMARY_FAMILY = 'jetbrains mono'

/** True when `family` is the engine's built-in primary (or unset) — no injection. */
export function isBundledPrimaryFamily(family: string | undefined): boolean {
  return !family || family.trim().toLowerCase() === BUNDLED_PRIMARY_FAMILY
}

/** Resolve `family` to its primary-face bytes and inject it via set_primary_font.
 *  Returns true iff a custom face was applied (the cell metrics changed, so the
 *  caller should reflow the grid). A bundled / unresolvable family returns false. */
export async function applyTerminalPrimaryFont(
  term: PrimaryFontInjectable,
  family: string | undefined
): Promise<boolean> {
  if (isBundledPrimaryFamily(family)) {
    return false
  }
  let bytes: Uint8Array | null = null
  try {
    bytes = await window.api.fonts.resolvePrimaryFont(family!.trim())
  } catch {
    return false
  }
  if (!bytes || bytes.length === 0) {
    return false
  }
  try {
    term.set_primary_font(new Uint8Array(bytes))
    return true
  } catch {
    // The host file wasn't a single loadable face (e.g. a .ttc collection) — keep
    // the bundled primary rather than rendering nothing.
    return false
  }
}
