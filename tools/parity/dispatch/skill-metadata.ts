// TS dispatch for the skill-metadata parity module: maps the shared vector
// function names to the real `src/shared/skill-metadata.ts` exports so the
// harness compares the live TS reference against the Rust port.

import { summarizeSkillMarkdown } from '../../../src/shared/skill-metadata'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'summarizeSkillMarkdown':
      return summarizeSkillMarkdown(input as string)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
