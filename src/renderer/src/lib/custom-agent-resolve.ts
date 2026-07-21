import type { CustomAgentProfile, GlobalSettings, TuiAgent } from '../../../shared/types'

/** Find a custom-agent profile by id within the user's settings. Returns
 *  `null` when settings are not yet hydrated or the id no longer matches a
 *  configured profile (handles the stale-default case where the user
 *  deleted the profile that was set as default). */
export function findCustomAgentProfile(
  settings: GlobalSettings | null | undefined,
  id: string | null | undefined
): CustomAgentProfile | null {
  if (!id || !settings?.customAgents) {
    return null
  }
  return settings.customAgents.find((p) => p.id === id) ?? null
}

/** Coerce the saved `defaultTuiAgent` preference into the renderer's two-piece
 *  selection model (built-in agent id + optional custom-profile id). Hides
 *  the union shape from the dozens of consumer sites that just want
 *  "what's the agent and is it custom?". */
export type ResolvedDefaultAgent =
  | { kind: 'auto' }
  | { kind: 'blank' }
  | { kind: 'builtin'; agent: TuiAgent }
  | { kind: 'custom'; agent: TuiAgent; profile: CustomAgentProfile }

export function resolveDefaultTuiAgentPreference(
  settings: GlobalSettings | null | undefined
): ResolvedDefaultAgent {
  const pref = settings?.defaultTuiAgent
  if (!pref) {
    return { kind: 'auto' }
  }
  if (pref === 'blank') {
    return { kind: 'blank' }
  }
  if (typeof pref === 'object' && pref !== null && pref.kind === 'custom') {
    const profile = findCustomAgentProfile(settings, pref.id)
    if (!profile) {
      // Why: a deleted custom profile must not strand the user with a
      // dead default. Falling back to auto matches the same behavior as
      // pickAgent() does for an uninstalled built-in.
      return { kind: 'auto' }
    }
    return { kind: 'custom', agent: profile.baseAgent, profile }
  }
  return { kind: 'builtin', agent: pref as TuiAgent }
}
