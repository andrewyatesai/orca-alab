// TS twin of the Rust orca-text workspace-name derivation (parity-proven at the
// wasm cutover): desktop runs it through the renderer orca-git wasm, which mobile
// (Hermes, no WebAssembly) cannot load, so the linked-item naming stays TS here.
import type {
  WorkspaceIntentName,
  WorkspaceIntentWorkItem
} from '../../../src/shared/workspace-name'
import {
  getWorkspaceSourceProvider,
  type WorkspaceSourceItemLike
} from '../../../src/shared/new-workspace/workspace-source'

function normalizeApostrophes(input: string): string {
  return input.replace(/[‘’]/g, "'")
}

// Why: contractions and possessives should not become stray `t` / `s` tokens
// in display names or extra hyphen segments in branch-safe workspace seeds.
function removeIntraWordApostrophes(input: string): string {
  return normalizeApostrophes(input).replace(/([\p{L}\p{N}])'(?=[\p{L}\p{N}])/gu, '$1')
}

function isWorkspaceNameWhitespace(code: number): boolean {
  return (
    code === 32 ||
    (code >= 9 && code <= 13) ||
    code === 160 ||
    code === 5760 ||
    (code >= 8192 && code <= 8202) ||
    code === 8232 ||
    code === 8233 ||
    code === 8239 ||
    code === 8287 ||
    code === 12288 ||
    code === 65279
  )
}

function foldWorkspaceNameWhitespaceToHyphen(input: string): string {
  let result = ''
  let pendingHyphen = false
  for (let index = 0; index < input.length; index += 1) {
    if (isWorkspaceNameWhitespace(input.charCodeAt(index))) {
      pendingHyphen = true
      continue
    }
    if (pendingHyphen) {
      result += '-'
      pendingHyphen = false
    }
    result += input[index]
  }
  return result
}

export function slugifyForWorkspaceName(input: string): string {
  const normalized = removeIntraWordApostrophes(input)
    .trim()
    .toLowerCase()
    .replace(/[\\/]+/g, '-')
  return (
    foldWorkspaceNameWhitespaceToHyphen(normalized)
      .replace(/[^a-z0-9._-]+/g, '-')
      .replace(/-+/g, '-')
      // Why: git check-ref-format rejects any ref containing `..`, so previews
      // must match the main-process sanitizer before workspace creation.
      .replace(/\.{2,}/g, '.')
      .replace(/^[.-]+|[.-]+$/g, '')
      .slice(0, 48)
      .replace(/[-._]+$/g, '')
  )
}

function getLinkedWorkItemTitleSubject(item: { title: string }): string {
  return item.title
    .trim()
    .replace(/^(?:issue|pr|pull request|mr|merge request)\s*[#!]?\d+\s*[:-]\s*/i, '')
    .replace(/^#\d+\s*[:-]\s*/, '')
    .replace(/\([#!]?\d+\)/g, '')
    .replace(/\b#\d+\b/g, '')
    .trim()
}

export function getLinkedWorkItemSuggestedName(item: { title: string }): string {
  const seed = getLinkedWorkItemTitleSubject(item) || item.title.trim()
  return slugifyForWorkspaceName(seed)
}

function escapeRegExp(input: string): string {
  return input.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

function workItemIdentity(item: WorkspaceIntentWorkItem): string {
  if (item.linearIdentifier) {
    return item.linearIdentifier.toUpperCase()
  }
  if (item.jiraIdentifier) {
    return item.jiraIdentifier.toUpperCase()
  }
  if (item.type === 'pr') {
    return `PR ${item.number}`
  }
  if (item.type === 'mr') {
    return `MR ${item.number}`
  }
  return `Issue ${item.number}`
}

export function getLinkedWorkItemWorkspaceName(
  item: WorkspaceIntentWorkItem
): WorkspaceIntentName | null {
  const identifier = item.linearIdentifier ?? item.jiraIdentifier
  let subject = getLinkedWorkItemTitleSubject(item) || item.title.trim()
  if (identifier) {
    subject = subject
      .replace(new RegExp(`^${escapeRegExp(identifier)}\\s*[:-]?\\s*`, 'i'), '')
      .trim()
  }
  const displayName = [identifier, subject].filter(Boolean).join(' ') || workItemIdentity(item)
  const seedName = slugifyForWorkspaceName(displayName)
  if (!seedName) {
    return null
  }
  return { displayName, seedName }
}

export function getLinearIssueWorkspaceName(issue: { identifier: string; title: string }): string {
  const key = slugifyForWorkspaceName(issue.identifier)
  const titleSlug = getLinkedWorkItemSuggestedName(issue)
  if (!key) {
    return titleSlug
  }
  let dedupedTitleSlug = titleSlug
  if (titleSlug === key) {
    dedupedTitleSlug = ''
  } else if (titleSlug.startsWith(`${key}-`)) {
    dedupedTitleSlug = titleSlug.slice(key.length + 1)
  }
  return slugifyForWorkspaceName([key, dedupedTitleSlug].filter(Boolean).join('-'))
}

// Mirrors the desktop renderer's wasm-backed getWorkspaceSourceName composition.
export function getWorkspaceSourceName(item: WorkspaceSourceItemLike): {
  seedName: string
  displayName: string
} {
  const normalized: WorkspaceIntentWorkItem = {
    ...item,
    provider: getWorkspaceSourceProvider(item)
  }
  const resolved = getLinkedWorkItemWorkspaceName(normalized)
  return {
    seedName: resolved?.seedName ?? getLinkedWorkItemSuggestedName(normalized),
    displayName: resolved?.displayName ?? item.title.trim()
  }
}
