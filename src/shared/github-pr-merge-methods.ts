// Logic moved to the Rust github-pr-merge-methods core (orca-dispatch); this file retains types + data only.
import type { GitHubPRMergeMethod } from './types'

export const GITHUB_PR_MERGE_METHODS = ['squash', 'merge', 'rebase'] as const

export const GITHUB_PR_MERGE_METHOD_LABELS: Record<GitHubPRMergeMethod, string> = {
  squash: 'Squash and merge',
  merge: 'Create merge commit',
  rebase: 'Rebase and merge'
}

export type GitHubPRMergeMethodOption = {
  method: GitHubPRMergeMethod
  label: string
}

export type GitHubPRMergeMethodPresentation = {
  defaultMethod: GitHubPRMergeMethod
  defaultLabel: string
  methods: GitHubPRMergeMethodOption[]
}
