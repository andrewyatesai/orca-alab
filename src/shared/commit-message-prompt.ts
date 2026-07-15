// Why: keeping the base prompt and assembly here (in shared) lets both the
// renderer (preview/tests) and main (actual generation) reach the exact same
// string without duplicating the wording.

const COMMIT_MESSAGE_BASE_PROMPT = `You are generating a single git commit message.
Read the staged diff below and produce the message.

Rules:
- First line: imperative mood, <= 72 chars, no trailing period.
- Optional body: blank line, then wrapped at 72 chars explaining WHY.
- Output ONLY the commit message - no preamble, no code fences, no quotes.
- Do not include "Co-authored-by" trailers - Orca appends them after generation when configured.

Staged diff:
\`\`\`diff
{{DIFF}}
\`\`\`
`

export {
  cleanGeneratedCommitMessage,
  excerptAgentFailureOutput
} from './commit-message-agent-output'

/** Builds the final prompt sent to the agent. The custom suffix is appended verbatim
 *  when non-empty so the user can override style (Conventional Commits, gitmoji, …). */
export function buildCommitPrompt(diff: string, customSuffix: string): string {
  const base = COMMIT_MESSAGE_BASE_PROMPT.replace('{{DIFF}}', diff)
  const trimmedSuffix = customSuffix.trim()
  if (!trimmedSuffix) {
    return base
  }
  return `${base}\n\nAdditional user prompt:\n${trimmedSuffix}`
}

// Diff truncation (byte budget + fair multi-file water-fill) is single-sourced in
// the Rust core (orca_agents::truncate_diff_for_prompt); production reaches it
// through build_commit_message_prompt. The former TS twin was deleted.

export const CUSTOM_PROMPT_PLACEHOLDER = '{prompt}'

export type TokenizeCustomCommandResult =
  | { ok: true; tokens: string[] }
  | { ok: false; error: string }

// Why: deliberately POSIX-shell-style only for *grouping* (single + double
// quotes, backslash escapes inside double quotes). We do NOT expand `$VAR`,
// command substitution, backticks, globs, or `~`. The user's intent is
// "spawn this exact CLI" — adding shell semantics on top would create
// surprising behavior across platforms (especially Windows) and a security
// surface we don't need.
export function tokenizeCustomCommandTemplate(template: string): TokenizeCustomCommandResult {
  const tokens: string[] = []
  let current = ''
  let inToken = false
  let quote: '"' | "'" | null = null
  let i = 0

  while (i < template.length) {
    const ch = template[i]
    if (quote) {
      if (ch === '\\' && quote === '"' && i + 1 < template.length) {
        current += template[i + 1]
        i += 2
        continue
      }
      if (ch === quote) {
        quote = null
        i++
        // Why: leaving a quoted region still keeps the token open — `a"b"c`
        // tokenizes as a single arg `abc`.
        inToken = true
        continue
      }
      current += ch
      i++
      continue
    }

    if (ch === '"' || ch === "'") {
      quote = ch
      inToken = true
      i++
      continue
    }

    if (ch === '\\' && i + 1 < template.length) {
      current += template[i + 1]
      inToken = true
      i += 2
      continue
    }

    if (/\s/.test(ch)) {
      if (inToken) {
        tokens.push(current)
        current = ''
        inToken = false
      }
      i++
      continue
    }

    current += ch
    inToken = true
    i++
  }

  if (quote) {
    return { ok: false, error: 'Unclosed quote in command template.' }
  }
  if (inToken) {
    tokens.push(current)
  }
  return { ok: true, tokens }
}
