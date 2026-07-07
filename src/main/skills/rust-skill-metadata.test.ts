import { describe, expect, it } from 'vitest'
import { summarizeSkillMarkdown } from './rust-skill-metadata'

// Ported from the deleted src/shared/skill-metadata.test.ts: the same
// expectations now run THROUGH the Rust orca-text parser via napi.

describe('summarizeSkillMarkdown (Rust napi)', () => {
  it('reads name and folded description from YAML frontmatter', () => {
    const summary = summarizeSkillMarkdown(`---
name: orca-cli
description: >-
  Use the orca CLI to drive a running editor;
  keep worktree comments current.
---

# Orca CLI
`)

    expect(summary).toEqual({
      name: 'orca-cli',
      description: 'Use the orca CLI to drive a running editor; keep worktree comments current.'
    })
  })

  it('falls back to heading and first paragraph when frontmatter is absent', () => {
    const summary = summarizeSkillMarkdown(`# Design Review

Use when reviewing UI implementation quality.
`)

    expect(summary).toEqual({
      name: 'Design Review',
      description: 'Use when reviewing UI implementation quality.'
    })
  })

  it('returns nulls for markdown with no frontmatter, heading, or paragraph', () => {
    expect(summarizeSkillMarkdown('')).toEqual({ name: null, description: null })
  })
})
