import type { AtermWorkerState } from './aterm-render-worker-protocol'

/** Two hover outcomes carry the same STATE payload — the main-thread underline span
 *  (row/startCol/endCol) plus the tooltip's url/kind — when every field matches; null≡null.
 *  Lets a hover sweep skip re-posting when it crosses cells that resolve to the same link. */
export function hoverLinkOutcomeEqual(
  a: AtermWorkerState['hoverLink'],
  b: AtermWorkerState['hoverLink']
): boolean {
  if (a === null || b === null) {
    return a === b
  }
  return (
    a.row === b.row &&
    a.startCol === b.startCol &&
    a.endCol === b.endCol &&
    a.url === b.url &&
    a.kind === b.kind
  )
}
