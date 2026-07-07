import type { SkillFrontmatterSummary } from '../../shared/skills'
import { requireRustGitBinding } from '../daemon/rust-git-addon'

/** Skill markdown frontmatter summary (name/description) — the Rust orca-text
 *  parser via napi (the shared TS parser was deleted). The Rust JSON omits
 *  absent fields; map them back to the nulls the discovery result shape uses. */
export function summarizeSkillMarkdown(markdown: string): SkillFrontmatterSummary {
  const parsed = JSON.parse(requireRustGitBinding().summarizeSkillMarkdown(markdown)) as {
    name?: string
    description?: string
  }
  return { name: parsed.name ?? null, description: parsed.description ?? null }
}
