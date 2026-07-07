// The workspace-name/seed derivation (slugifyForWorkspaceName,
// getLinkedWorkItemSuggestedName, getLinkedWorkItemWorkspaceName,
// getWorkspaceIntentName, getLinearIssueWorkspaceName, resolveWorkspaceCreateName)
// moved to the Rust orca-text core: the renderer drives it through the orca-git
// wasm (src/renderer/src/lib/git-wasm/workspace-name.ts). This shared module
// keeps only the types those boundaries and the parity dispatch reference.

export type WorkspaceIntentWorkItem = {
  type: 'issue' | 'pr' | 'mr'
  number: number
  title: string
  provider?: 'github' | 'gitlab' | 'linear' | 'jira'
  linearIdentifier?: string
  jiraIdentifier?: string
}

export type WorkspaceIntentName = {
  displayName: string
  seedName: string
}
