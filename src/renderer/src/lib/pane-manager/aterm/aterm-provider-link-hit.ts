// Hit-testing for xterm-style link providers (createFilePathLinkProvider,
// createTerminalHandleLinkProvider). The engine detects URLs/OSC-8/file paths
// natively; the providers cover the app-level links it can't know about
// (term_/task_ handles, cwd-resolved file paths with existence probes), so the
// link input consults them only where the engine reports no link.

import type { IBufferRange, ILink, ILinkProvider } from './terminal-types'

/** The facade's live registered link providers (registerLinkProvider), consulted
 *  when the engine reports no link at a cell. */
export type AtermLinkProviderSource = () => readonly ILinkProvider[]

// A provider that never calls back (createFilePathLinkProvider drops the
// callback when the line changed under it) must not wedge the providers behind
// it, so each provider's answer is raced against this deadline.
const PROVIDER_ANSWER_DEADLINE_MS = 2000

/** True when the 1-based inclusive buffer range contains cell (x, y). */
export function providerRangeContainsCell(range: IBufferRange, x: number, y: number): boolean {
  if (y < range.start.y || y > range.end.y) {
    return false
  }
  if (y === range.start.y && x < range.start.x) {
    return false
  }
  return !(y === range.end.y && x > range.end.x)
}

function askProvider(
  provider: ILinkProvider,
  bufferLineNumber: number
): Promise<ILink[] | undefined> {
  return new Promise((resolve) => {
    const deadline = setTimeout(() => resolve(undefined), PROVIDER_ANSWER_DEADLINE_MS)
    try {
      provider.provideLinks(bufferLineNumber, (links) => {
        clearTimeout(deadline)
        resolve(links ?? undefined)
      })
    } catch {
      clearTimeout(deadline)
      resolve(undefined)
    }
  })
}

/** Resolve the provider link at 1-based buffer cell (x, bufferLineNumber), or
 *  null. Providers run in registration order and the first one that yields a
 *  link containing the cell wins (xterm's linkifier precedence). */
export async function resolveProviderLinkAt(
  providers: readonly ILinkProvider[],
  bufferLineNumber: number,
  x: number
): Promise<ILink | null> {
  for (const provider of providers) {
    const links = await askProvider(provider, bufferLineNumber)
    const hit = links?.find((link) => providerRangeContainsCell(link.range, x, bufferLineNumber))
    if (hit) {
      return hit
    }
  }
  return null
}
