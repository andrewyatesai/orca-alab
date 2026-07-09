import { resolveTerminalFontWeights } from './rust-terminal-fonts'

// Pure face selection for the aterm engine's set_primary_font / set_bold_font:
// given the named styles a family actually ships on the host, pick the face
// closest to the user's numeric terminalFontWeight for body text and a REAL
// heavier face for SGR bold. The engine rasterizes injected faces (not CSS
// weights), so numeric weights can only map onto styles the family provides —
// a missing bold face returns null and the engine keeps synthetic embolden.

/** One discovered face of a family: its named style + the host font file path. */
export type FontFaceCandidate = { style: string; path: string }

export type SelectedFontFaces = {
  primary: FontFaceCandidate | null
  bold: FontFaceCandidate | null
}

// Canonical CSS-ish weight per named style, keyed by the style with spaces/
// hyphens stripped so "Extra Bold", "ExtraBold" and "extra-bold" all match.
const WEIGHT_BY_STYLE_NAME: Record<string, number> = {
  thin: 100,
  hairline: 100,
  extralight: 200,
  ultralight: 200,
  light: 300,
  regular: 400,
  book: 400,
  roman: 400,
  normal: 400,
  text: 400,
  // Fira Code's between-Regular-and-Medium face.
  retina: 450,
  medium: 500,
  semibold: 600,
  demibold: 600,
  demi: 600,
  bold: 700,
  extrabold: 800,
  ultrabold: 800,
  heavy: 900,
  black: 900,
  extrablack: 950,
  ultrablack: 950
}

const ITALIC_RE = /\b(?:italic|oblique)\b/i
// Width variants never render terminal body text; a "Condensed Bold" face must
// not win over the family's normal-width faces (or be faked in when none exist).
const WIDTH_VARIANT_RE = /\b(?:condensed|narrow|compressed|expanded|extended|wide)\b/i

// Anything below SemiBold would not read as bold next to the primary face.
const BOLD_MIN_WEIGHT = 600

/** Weight of a named style, or null when the style is italic, a width variant,
 *  or not a recognizable single weight name (e.g. a sibling family suffix like
 *  "NL Bold" — matching it would silently swap the family). Empty = Regular. */
export function fontStyleWeight(style: string): number | null {
  const trimmed = style.trim()
  if (trimmed.length === 0) {
    return 400
  }
  if (ITALIC_RE.test(trimmed) || WIDTH_VARIANT_RE.test(trimmed)) {
    return null
  }
  return WEIGHT_BY_STYLE_NAME[trimmed.toLowerCase().replace(/[\s-]+/g, '')] ?? null
}

function isTtcPath(path: string): boolean {
  return path.toLowerCase().endsWith('.ttc')
}

type WeightedFace = { candidate: FontFaceCandidate; weight: number }

/** Closest face by weight; ties prefer a single-face file (the engine's glyph
 *  loader reads one face, so .ttc collections are a last resort), then the
 *  lighter weight (stable, and favors Regular over a heavier neighbor). */
function pickClosest(faces: WeightedFace[], target: number): WeightedFace | null {
  let best: WeightedFace | null = null
  for (const face of faces) {
    if (best === null) {
      best = face
      continue
    }
    const delta = Math.abs(face.weight - target)
    const bestDelta = Math.abs(best.weight - target)
    if (
      delta < bestDelta ||
      (delta === bestDelta && isTtcPath(best.candidate.path) && !isTtcPath(face.candidate.path)) ||
      (delta === bestDelta &&
        isTtcPath(best.candidate.path) === isTtcPath(face.candidate.path) &&
        face.weight < best.weight)
    ) {
      best = face
    }
  }
  return best
}

/** Pick the primary + bold faces for the user's terminalFontWeight (default 500).
 *  Primary: the closest named style; families without recognizable style names
 *  fall back to the first single-face file (today's Regular-pick behavior).
 *  Bold: the style closest to the derived bold weight, required to be >= SemiBold,
 *  heavier than the primary, and a DIFFERENT file (the engine loads one face per
 *  file, so a .ttc that holds both styles cannot provide a real bold). Null bold
 *  keeps the engine's synthetic embolden. */
export function selectTerminalFontFaces(
  candidates: readonly FontFaceCandidate[],
  fontWeight?: number | null
): SelectedFontFaces {
  const weights = resolveTerminalFontWeights(fontWeight)
  const weighted: WeightedFace[] = []
  for (const candidate of candidates) {
    const weight = fontStyleWeight(candidate.style)
    if (weight !== null) {
      weighted.push({ candidate, weight })
    }
  }

  const primaryPick = pickClosest(weighted, weights.fontWeight)
  if (!primaryPick) {
    const fallback = candidates.find((c) => !isTtcPath(c.path)) ?? candidates[0] ?? null
    return { primary: fallback, bold: null }
  }

  const boldEligible = weighted.filter(
    (face) => face.weight >= BOLD_MIN_WEIGHT && face.weight > primaryPick.weight
  )
  const boldPick = pickClosest(boldEligible, weights.fontWeightBold)
  return {
    primary: primaryPick.candidate,
    bold:
      boldPick && boldPick.candidate.path !== primaryPick.candidate.path ? boldPick.candidate : null
  }
}
