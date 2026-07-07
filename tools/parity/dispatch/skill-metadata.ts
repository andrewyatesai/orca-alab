// TS dispatch for the skill-metadata parity module. The shared TS parser was
// DELETED (the Rust orca-text core is the sole impl, napi in main), so this
// adapter drives the napi binding through the same null-mapping wrapper the
// main process uses: the vectors' recorded goldens now pin that surface
// absolutely. Requires the built addon.

import { summarizeSkillMarkdown } from '../../../src/main/skills/rust-skill-metadata'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'summarizeSkillMarkdown':
      return summarizeSkillMarkdown(input as string)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
