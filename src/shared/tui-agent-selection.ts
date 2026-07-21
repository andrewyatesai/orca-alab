import type { CustomAgentProfile, TuiAgent } from './types'
import { isTuiAgent } from './tui-agent-config'

/** Collapse a saved `defaultTuiAgent` preference to its built-in base for
 *  consumers that only understand built-ins: a `{ kind: 'custom', id }` entry
 *  resolves to the profile's baseAgent, or to null (auto) when the profile no
 *  longer exists / no roster was provided. */
export function collapseDefaultTuiAgentToBuiltin(
  pref: TuiAgent | 'blank' | { kind: 'custom'; id: string } | null | undefined,
  customAgents?: readonly CustomAgentProfile[] | null
): TuiAgent | 'blank' | null | undefined {
  if (pref && typeof pref === 'object') {
    return customAgents?.find((profile) => profile.id === pref.id)?.baseAgent ?? null
  }
  return pref
}

// Keep this order in sync with the desktop agent catalog. It defines the
// automatic fallback priority when the user has not chosen a default agent.
export const TUI_AGENT_AUTO_PICK_ORDER = [
  'claude',
  'claude-agent-teams',
  'openclaude',
  'codex',
  'grok',
  'copilot',
  'opencode',
  'mimo-code',
  'ante',
  'pi',
  'omp',
  'gemini',
  'antigravity',
  'aider',
  'goose',
  'amp',
  'kilo',
  'kiro',
  'crush',
  'aug',
  'autohand',
  'cline',
  'codebuff',
  'command-code',
  'continue',
  'cursor',
  'droid',
  'kimi',
  'mistral-vibe',
  'qwen-code',
  'rovo',
  'hermes',
  'devin',
  'openclaw'
] as const satisfies readonly TuiAgent[]

// Why: fresh installs should expose Claude Agent Teams in agent pickers; the
// persistence migration separately preserves the old hidden default for legacy profiles.
export const DEFAULT_DISABLED_TUI_AGENTS = [] as const satisfies readonly TuiAgent[]

export function pickTuiAgent(
  preferred: TuiAgent | 'blank' | null | undefined,
  detected: Iterable<TuiAgent>,
  disabled?: Iterable<unknown> | null
): TuiAgent | null {
  if (preferred === 'blank') {
    return null
  }
  const disabledSet = new Set(normalizeDisabledTuiAgents(disabled))
  const detectedSet = detected instanceof Set ? detected : new Set(detected)
  if (preferred && detectedSet.has(preferred) && !disabledSet.has(preferred)) {
    return preferred
  }
  for (const agent of TUI_AGENT_AUTO_PICK_ORDER) {
    if (detectedSet.has(agent) && !disabledSet.has(agent)) {
      return agent
    }
  }
  return null
}

export function normalizeDisabledTuiAgents(value: unknown): TuiAgent[] {
  if (!Array.isArray(value)) {
    return []
  }
  const seen = new Set<TuiAgent>()
  for (const item of value) {
    if (isTuiAgent(item)) {
      seen.add(item)
    }
  }
  return [...seen]
}

export function isTuiAgentEnabled(agent: TuiAgent, disabled?: Iterable<unknown> | null): boolean {
  return !normalizeDisabledTuiAgents(disabled).includes(agent)
}

export function filterEnabledTuiAgents<T extends TuiAgent>(
  agents: Iterable<T>,
  disabled?: Iterable<unknown> | null
): T[] {
  const disabledSet = new Set(normalizeDisabledTuiAgents(disabled))
  return [...agents].filter((agent) => !disabledSet.has(agent))
}
