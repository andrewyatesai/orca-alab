// Why: Source Control unmounts on right-sidebar tab switch (upstream #9403); keep transient disclosure
// state (collapsed sections/tree dirs, filter bar) in a module-scoped session cache keyed by worktree
// so remount and worktree switches restore the user's choices instead of resetting to defaults.

export const DEFAULT_COLLAPSED_SECTION_IDS = ['history'] as const

export type WorktreeDisclosureState = {
  collapsedSections: Set<string>
  collapsedTreeDirs: Set<string>
  filterExpanded: boolean
}

export type DisclosureStateByWorktree = Record<string, WorktreeDisclosureState>

let sessionDisclosureByWorktree: DisclosureStateByWorktree = {}

export function loadSessionDisclosureState(): DisclosureStateByWorktree {
  return sessionDisclosureByWorktree
}

export function saveSessionDisclosureState(next: DisclosureStateByWorktree): void {
  sessionDisclosureByWorktree = next
}

export function createDefaultDisclosureState(): WorktreeDisclosureState {
  return {
    collapsedSections: new Set(DEFAULT_COLLAPSED_SECTION_IDS),
    collapsedTreeDirs: new Set(),
    filterExpanded: false
  }
}

export function readDisclosureStateForWorktree(
  store: DisclosureStateByWorktree,
  worktreeId: string | null | undefined
): WorktreeDisclosureState {
  if (!worktreeId) {
    return createDefaultDisclosureState()
  }
  return store[worktreeId] ?? createDefaultDisclosureState()
}

export function writeDisclosureStateForWorktree(
  store: DisclosureStateByWorktree,
  worktreeId: string,
  update: Partial<WorktreeDisclosureState>
): DisclosureStateByWorktree {
  const current = store[worktreeId] ?? createDefaultDisclosureState()
  return {
    ...store,
    [worktreeId]: { ...current, ...update }
  }
}
