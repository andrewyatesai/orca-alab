// TS dispatch for the workspace-statuses parity module: maps the shared vector
// function names to the real `src/shared/workspace-statuses.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  getDefaultWorkspaceStatusId,
  getWorkspaceStatusFromGroupKey,
  getWorkspaceStatusGroupKey,
  isWorkspaceStatusId,
  normalizeWorkspaceStatuses
} from '../../../src/shared/workspace-statuses'
import type { WorkspaceStatusDefinition } from '../../../src/shared/types'

type StatusList = readonly WorkspaceStatusDefinition[]

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'normalizeWorkspaceStatuses':
      return normalizeWorkspaceStatuses(input)
    case 'isWorkspaceStatusId': {
      const { value, statuses } = input as { value: string; statuses: StatusList }
      return isWorkspaceStatusId(value, statuses)
    }
    case 'getDefaultWorkspaceStatusId':
      return getDefaultWorkspaceStatusId(input as StatusList)
    case 'getWorkspaceStatusGroupKey':
      return getWorkspaceStatusGroupKey(input as string)
    case 'getWorkspaceStatusFromGroupKey': {
      const { groupKey, statuses } = input as { groupKey: string; statuses: StatusList }
      return getWorkspaceStatusFromGroupKey(groupKey, statuses)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
