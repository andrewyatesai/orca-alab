// Main-process pull-request field generator, driven by the Rust orca-agents core
// via napi (the shared TS bodies were deleted). The renderer previews the prompt
// through wasm; here we both build the prompt and parse the agent's reply — one
// source of truth for the base/title/body/draft contract.
import { requireRustGitBinding } from '../daemon/rust-git-addon'
import type {
  PullRequestDraftContext,
  GeneratedPullRequestFields
} from '../../shared/pull-request-generation'

export function buildPullRequestFieldsPrompt(
  context: PullRequestDraftContext,
  customPrompt: string
): string {
  return requireRustGitBinding().buildPullRequestFieldsPrompt(JSON.stringify(context), customPrompt)
}

export function parseGeneratedPullRequestFields(
  raw: string,
  fallback: Pick<PullRequestDraftContext, 'base' | 'currentTitle' | 'currentBody' | 'currentDraft'>
): GeneratedPullRequestFields {
  const result = JSON.parse(
    requireRustGitBinding().parseGeneratedPullRequestFields(raw, JSON.stringify(fallback))
  ) as { ok: true; fields: GeneratedPullRequestFields } | { ok: false; error: string }
  // Preserve the old throw-on-unparseable contract so callers' try/catch holds.
  if (!result.ok) {
    throw new Error(result.error)
  }
  return result.fields
}
