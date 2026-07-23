// Resolves a context-menu file target's raw matched span to an absolute path for
// "Reveal in Finder / File Manager" (#9279) — the same candidate-extraction +
// cwd/home resolution the pane's click-to-open file opener uses, minus the open.

import {
  extractTerminalFileLinkCandidates,
  resolveTerminalFileLink
} from '../../terminal-links'
import type { AtermLinkContext } from './aterm-url-link-routing'

/** Absolute path for a raw file-link span, resolved against the pane's live
 *  cwd + home from the late-bound link context; null when the context has not
 *  been bound yet or the span doesn't parse/resolve. */
export function resolveAtermFileLinkAbsolutePath(
  rawPathText: string,
  context: AtermLinkContext | undefined
): string | null {
  const cwd = context?.getStartupCwd?.()
  if (!cwd) {
    return null
  }
  const [parsed] = extractTerminalFileLinkCandidates(rawPathText)
  if (!parsed) {
    return null
  }
  return resolveTerminalFileLink(parsed, cwd, context?.terminalHomePath)?.absolutePath ?? null
}
