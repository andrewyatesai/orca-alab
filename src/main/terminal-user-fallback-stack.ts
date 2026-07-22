import { resolveTerminalFontFaceBytes } from './system-fonts'

// The USER half of the terminal fallback chain (PC-8367): the ordered
// terminalFontFallbackFamilies setting resolved to face bytes through the same
// family→bytes seam terminalFontFamily uses. The OS-discovered half lives in
// terminal-fallback-fonts.ts; getTerminalFallbackFonts assembles the two.

/** One resolved user-stack face; `path` supports de-dup against the OS half. */
export type UserFallbackFace = { family: string; bytes: Uint8Array; path: string | null }

// Memoized per family-list (key = families joined) so a settings change
// naturally misses the cache — no invalidation hook needed.
const cachedStacks = new Map<string, Promise<UserFallbackFace[]>>()

/** Trimmed, non-empty family names in the user's order. */
export function normalizeUserFallbackFamilies(families: readonly string[] | undefined): string[] {
  return (families ?? []).map((family) => family.trim()).filter(Boolean)
}

/**
 * Resolve the user's ordered fallback families to face bytes (weight 400).
 * Unresolvable families are skipped (issue requirement: unavailable font →
 * preserve current behavior); duplicates within the stack collapse to the first
 * occurrence. Results are memoized per family list for the process lifetime.
 */
export function loadUserFallbackStack(families: readonly string[]): Promise<UserFallbackFace[]> {
  const key = families.join('\n')
  let stack = cachedStacks.get(key)
  if (!stack) {
    stack = resolveUserFallbackStack(families)
    cachedStacks.set(key, stack)
  }
  return stack
}

async function resolveUserFallbackStack(families: readonly string[]): Promise<UserFallbackFace[]> {
  const faces: UserFallbackFace[] = []
  const seenPaths = new Set<string>()
  for (const family of families) {
    const resolved = await resolveTerminalFontFaceBytes(family, 400)
    if (!resolved.primary) {
      continue
    }
    if (resolved.primaryPath) {
      if (seenPaths.has(resolved.primaryPath)) {
        continue
      }
      seenPaths.add(resolved.primaryPath)
    }
    faces.push({ family, bytes: resolved.primary, path: resolved.primaryPath })
  }
  return faces
}
