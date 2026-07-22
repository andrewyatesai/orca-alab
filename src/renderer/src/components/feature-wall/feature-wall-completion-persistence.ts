import {
  FEATURE_WALL_STEP_IDS,
  FEATURE_WALL_WORKFLOW_IDS,
  type FeatureWallStepId,
  type FeatureWallWorkflowId
} from '../../../../shared/feature-wall-workflows'

const PERSISTED_WORKFLOW_IDS = new Set<FeatureWallWorkflowId>(FEATURE_WALL_WORKFLOW_IDS)
const PERSISTED_STEP_IDS = new Set<FeatureWallStepId>(FEATURE_WALL_STEP_IDS)
const VISITED_WORKFLOWS_STORAGE_KEY = 'orca.featureWall.visitedWorkflows.v2'
const VISITED_STEPS_STORAGE_KEY = 'orca.featureWall.visitedSteps.v2'

function normalizeIds<T extends string>(value: unknown, validIds: ReadonlySet<T>): T[] {
  if (!Array.isArray(value)) {
    return []
  }
  const seen = new Set<T>()
  for (const item of value) {
    if (typeof item === 'string' && validIds.has(item as T)) {
      seen.add(item as T)
    }
  }
  return [...seen]
}

export function normalizeFeatureWallVisitedWorkflows(value: unknown): FeatureWallWorkflowId[] {
  return normalizeIds(value, PERSISTED_WORKFLOW_IDS)
}

export function normalizeFeatureWallVisitedSteps(value: unknown): FeatureWallStepId[] {
  return normalizeIds(value, PERSISTED_STEP_IDS)
}

function readPersistedIds<T extends string>(
  storageKey: string,
  normalize: (value: unknown) => T[]
): Set<T> {
  if (typeof localStorage === 'undefined') {
    return new Set()
  }
  try {
    return new Set(normalize(JSON.parse(localStorage.getItem(storageKey) ?? '[]')))
  } catch {
    return new Set()
  }
}

export function readPersistedVisitedWorkflows(): Set<FeatureWallWorkflowId> {
  return readPersistedIds(VISITED_WORKFLOWS_STORAGE_KEY, normalizeFeatureWallVisitedWorkflows)
}

export function readPersistedVisitedSteps(): Set<FeatureWallStepId> {
  return readPersistedIds(VISITED_STEPS_STORAGE_KEY, normalizeFeatureWallVisitedSteps)
}

function persistVisitedId<T extends string>(
  storageKey: string,
  id: T,
  validIds: ReadonlySet<T>,
  read: () => Set<T>
): void {
  if (!validIds.has(id) || typeof localStorage === 'undefined') {
    return
  }
  try {
    const next = read()
    next.add(id)
    localStorage.setItem(storageKey, JSON.stringify([...next]))
  } catch {
    // localStorage can be unavailable in hardened contexts; React state still
    // keeps progress stable for the current walkthrough session.
  }
}

export function persistVisitedWorkflow(id: FeatureWallWorkflowId): void {
  persistVisitedId(
    VISITED_WORKFLOWS_STORAGE_KEY,
    id,
    PERSISTED_WORKFLOW_IDS,
    readPersistedVisitedWorkflows
  )
}

export function persistVisitedStep(id: FeatureWallStepId): void {
  persistVisitedId(VISITED_STEPS_STORAGE_KEY, id, PERSISTED_STEP_IDS, readPersistedVisitedSteps)
}
