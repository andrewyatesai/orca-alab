import { getAgentCatalog } from '@/lib/agent-catalog'
import type { CustomAgentProfile, TuiAgent } from '../../../../shared/types'

/** Per-row edit state for a custom agent profile: env map flattened to ordered
 *  key/value pairs so the settings UI can edit rows without object churn.
 *  Each pair carries a stable `id` so React keys the row by identity, not array
 *  index — deleting/reordering a middle row must unmount that row's inputs
 *  rather than shift later rows' caret/selection/IME state into stale nodes. */
export type CustomAgentEnvPair = { id: string; key: string; value: string }

export type CustomAgentDraftRow = Omit<CustomAgentProfile, 'env'> & {
  envPairs: CustomAgentEnvPair[]
}

/** Builds an env pair with a fresh stable id for keying its row. */
export function newCustomAgentEnvPair(key = '', value = ''): CustomAgentEnvPair {
  return { id: globalThis.crypto.randomUUID(), key, value }
}

export function customAgentProfileToDraft(profile: CustomAgentProfile): CustomAgentDraftRow {
  return {
    id: profile.id,
    label: profile.label,
    baseAgent: profile.baseAgent,
    command: profile.command,
    envPairs: Object.entries(profile.env ?? {}).map(([key, value]) =>
      newCustomAgentEnvPair(key, value)
    )
  }
}

/** Returns null while the draft is incomplete (missing label/command) so
 *  half-filled rows never commit to settings. */
export function customAgentDraftToProfile(draft: CustomAgentDraftRow): CustomAgentProfile | null {
  const label = draft.label.trim()
  const command = draft.command.trim()
  if (!label || !command) {
    return null
  }
  const env: Record<string, string> = {}
  for (const pair of draft.envPairs) {
    const key = pair.key.trim()
    if (!key) {
      continue
    }
    env[key] = pair.value
  }
  const profile: CustomAgentProfile = {
    id: draft.id,
    label,
    baseAgent: draft.baseAgent,
    command
  }
  if (Object.keys(env).length > 0) {
    profile.env = env
  }
  return profile
}

export function newCustomAgentDraftFor(baseAgent: TuiAgent): CustomAgentDraftRow {
  const entry = getAgentCatalog().find((a) => a.id === baseAgent)
  return {
    id: globalThis.crypto.randomUUID(),
    label: '',
    baseAgent,
    command: entry?.cmd ?? baseAgent,
    envPairs: [newCustomAgentEnvPair()]
  }
}
