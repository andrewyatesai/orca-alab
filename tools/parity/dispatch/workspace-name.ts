// TS dispatch for the workspace-name parity module: maps the shared vector
// function names to the real `src/shared/workspace-name.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  getLinkedWorkItemSuggestedName,
  getLinkedWorkItemWorkspaceName,
  getWorkspaceIntentName,
  slugifyForWorkspaceName,
  type WorkspaceIntentWorkItem
} from '../../../src/shared/workspace-name'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'slugifyForWorkspaceName':
      return slugifyForWorkspaceName(input as string)
    case 'getLinkedWorkItemSuggestedName':
      return getLinkedWorkItemSuggestedName(input as { title: string })
    case 'getWorkspaceIntentName':
      return getWorkspaceIntentName(
        input as {
          sourceText?: string
          workItem?: WorkspaceIntentWorkItem | null
          fallbackName?: string
        }
      )
    case 'getLinkedWorkItemWorkspaceName':
      return getLinkedWorkItemWorkspaceName(input as WorkspaceIntentWorkItem)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
