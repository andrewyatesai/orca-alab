// Honor the user's terminalFontFamily (+ numeric terminalFontWeight): resolve the
// family's faces on the host (window.api.fonts.resolveTerminalFontFaces) and inject
// the weight-closest face as the engine's primary (set_primary_font) plus the
// family's REAL bold style, when it ships one, for SGR bold (set_bold_font). The
// bundled JetBrains Mono is the engine's built-in primary, so a JetBrains-Mono (or
// unset) family needs no injection. Graceful: any failure (unresolvable family,
// unloadable blob) keeps the bundled face / the engine's synthetic embolden —
// never throws.

type PrimaryFontInjectable = {
  set_primary_font: (bytes: Uint8Array) => void
  set_bold_font: (bytes: Uint8Array) => void
}

const BUNDLED_PRIMARY_FAMILY = 'jetbrains mono'

/** True when `family` is the engine's built-in primary (or unset) — no injection. */
export function isBundledPrimaryFamily(family: string | undefined): boolean {
  return !family || family.trim().toLowerCase() === BUNDLED_PRIMARY_FAMILY
}

/** Resolve `family` to its face bytes and inject them (primary + optional bold).
 *  Returns true iff a custom primary face was applied (the cell metrics changed,
 *  so the caller should reflow the grid). A bundled / unresolvable family returns
 *  false. */
export async function applyTerminalPrimaryFont(
  term: PrimaryFontInjectable,
  family: string | undefined,
  fontWeight?: number
): Promise<boolean> {
  if (isBundledPrimaryFamily(family)) {
    return false
  }
  let faces: { primary: Uint8Array | null; bold: Uint8Array | null }
  try {
    faces = await window.api.fonts.resolveTerminalFontFaces(family!.trim(), fontWeight)
  } catch {
    return false
  }
  if (!faces?.primary || faces.primary.length === 0) {
    return false
  }
  try {
    term.set_primary_font(new Uint8Array(faces.primary))
  } catch {
    // The host file wasn't a single loadable face (e.g. a .ttc collection) — keep
    // the bundled primary rather than rendering nothing.
    return false
  }
  // Bold is best-effort: absent or unloadable bold bytes keep the engine's
  // synthetic embolden, so SGR bold still reads bold.
  if (faces.bold && faces.bold.length > 0) {
    try {
      term.set_bold_font(new Uint8Array(faces.bold))
    } catch {
      /* unloadable bold face — synthetic embolden covers it */
    }
  }
  return true
}
