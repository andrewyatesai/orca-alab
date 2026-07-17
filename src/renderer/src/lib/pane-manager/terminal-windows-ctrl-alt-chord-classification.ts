// Why: Windows Ctrl+Alt chords can be AltGr composition on layouts where AltGr
// is Ctrl+Alt-aliased (#8734/#8810). Chromium rewrites a COMPOSING Ctrl+Alt
// press to the AltGraph modifier (crbug 762557), so a chord still reporting
// Ctrl+Alt without AltGraph cannot compose text and must reach the terminal
// key encoder. The fork consumes these predicates in aterm-key-encoding.ts
// (upstream patches xterm's `_isThirdLevelShift` internals instead).

type ThirdLevelShiftKeyboardEvent = Pick<KeyboardEvent, 'ctrlKey' | 'altKey' | 'metaKey'> & {
  getModifierState?: (keyArg: string) => boolean
}

/**
 * Returns whether a Windows Ctrl+Alt chord is genuine keyboard input rather
 * than AltGr composition, and must therefore reach the key encoder.
 *
 * When a Ctrl+Alt keydown composes a printable character on the active
 * layout, Chromium replaces the Control+Alt modifiers with AltGraph
 * (crbug 762557), so a chord still reporting Ctrl+Alt without AltGraph
 * cannot compose text.
 */
export function isGenuineWindowsCtrlAltChord(event: ThirdLevelShiftKeyboardEvent): boolean {
  return (
    event.ctrlKey && event.altKey && !event.metaKey && event.getModifierState?.('AltGraph') !== true
  )
}

/** Returns whether this client's AltGraph modifier state is trustworthy. */
export function shouldRepairWindowsCtrlAltChords(userAgent: string): boolean {
  // Why: only Chromium rewrites composing Ctrl+Alt presses to AltGraph. Paired
  // web clients on Firefox keep stock classification so Ctrl+Alt-alias AltGr
  // typing there is never misread as a chord.
  return userAgent.includes('Windows') && userAgent.includes('Chrome/')
}
