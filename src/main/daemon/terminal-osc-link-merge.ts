import type { TerminalOscLinkRange } from '../../shared/terminal-osc-link-ranges'

const linkKey = (r: TerminalOscLinkRange): string => `${r.row}:${r.startCol}:${r.endCol}:${r.uri}`

/** Merge checkpoint-restored OSC-8 link ranges with the engine's live ranges,
 *  clamped to the current width and de-duplicated, so a restored buffer keeps
 *  its clickable links without double-counting ones the live grid already has. */
export function mergeRestoredOscLinks(
  live: TerminalOscLinkRange[],
  restored: TerminalOscLinkRange[],
  cols: number
): TerminalOscLinkRange[] {
  if (restored.length === 0) {
    return live
  }
  const merged = [...live]
  const seen = new Set(live.map(linkKey))
  for (const link of restored) {
    const clamped: TerminalOscLinkRange = {
      ...link,
      startCol: Math.min(link.startCol, cols),
      endCol: Math.min(link.endCol, cols)
    }
    const key = linkKey(clamped)
    if (!seen.has(key)) {
      seen.add(key)
      merged.push(clamped)
    }
  }
  return merged
}
