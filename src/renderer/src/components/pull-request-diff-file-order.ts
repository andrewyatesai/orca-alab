import {
  buildSourceControlTree,
  compactSourceControlTree,
  flattenSourceControlTree,
  type SourceControlTreeEntry
} from '@/components/right-sidebar/source-control-tree'

const NO_COLLAPSED_KEYS: ReadonlySet<string> = new Set()

/**
 * Order diff file entries by the same directory-grouped, path-sorted DFS the
 * combined-diff file tree renders.
 *
 * Why: the PR "Files changed" view built its diff sections straight from the raw
 * GitHub API order while the left file tree re-sorted the identical entries
 * (buildSourceControlTree groups directories-first and path-sorts files), so a
 * file's tree-row position did not match its diff-scroll position (issue #9485).
 * Building sections from this tree order keeps the two panes in lockstep.
 */
export function orderEntriesByFileTree<Entry extends SourceControlTreeEntry>(
  entries: readonly Entry[]
): Entry[] {
  const roots = compactSourceControlTree(buildSourceControlTree('diff', [...entries]))
  return flattenSourceControlTree(roots, NO_COLLAPSED_KEYS).flatMap((node) =>
    node.type === 'file' ? [node.entry] : []
  )
}
