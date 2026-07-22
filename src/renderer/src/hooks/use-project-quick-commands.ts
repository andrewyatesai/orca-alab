import { useEffect, useMemo, useState } from 'react'
import { useAppStore } from '@/store'
import { useRepoById } from '@/store/selectors'
import { checkRuntimeHooks, type HookCheckResult } from '@/runtime/runtime-hooks-client'
import { ensureHooksConfirmed, settingsForHookRepoOwner } from '@/lib/ensure-hooks-confirmed'
import { hashOrcaHookScript, isSharedOrcaCommandTrusted } from '@/lib/orca-hook-trust'
import { resolveHookCommandSourcePolicy } from '@/lib/git-wasm/hook-command-source-policy'
import { getSharedCommandTrustContent } from '../../../shared/orca-yaml-trust-content'
import { projectQuickCommandsForRepo } from '../../../shared/project-quick-commands'
import type { Repo, TerminalQuickCommand } from '../../../shared/types'

/**
 * Delivery of orca.yaml project quick commands (#8481) to the renderer over the
 * existing hooks pipe (`hooks:check` / `repo.hooksCheck`) — no new IPC. Every
 * failure path resolves to "no project commands" (fail closed): repo-controlled
 * yaml must never surface runnable entries Orca could not inspect and hash.
 */

export type ProjectQuickCommandsSnapshot = {
  commands: TerminalQuickCommand[]
  /** Hash of the FULL shared-command trust content the commands arrived in; the dispatch gate compares it to the repo's trust record. */
  sharedTrustContentHash: string | null
}

const EMPTY_SNAPSHOT: ProjectQuickCommandsSnapshot = { commands: [], sharedTrustContentHash: null }

const snapshotCache = new Map<string, Promise<ProjectQuickCommandsSnapshot>>()
const invalidationListeners = new Set<() => void>()

export function invalidateProjectQuickCommands(repoId?: string): void {
  if (repoId) {
    snapshotCache.delete(repoId)
  } else {
    snapshotCache.clear()
  }
  for (const listener of invalidationListeners) {
    listener()
  }
}

/** Local-only repos must not surface committed orca.yaml commands at all. */
export function isProjectQuickCommandSourceLocalOnly(
  repo: Pick<Repo, 'hookSettings'> | null | undefined
): boolean {
  return (
    resolveHookCommandSourcePolicy(repo?.hookSettings?.commandSourcePolicy, {
      hasLocalScript: Boolean(repo?.hookSettings?.scripts?.setup?.trim())
    }) === 'local-only'
  )
}

export async function resolveProjectQuickCommandsSnapshot(
  repoId: string,
  result: HookCheckResult
): Promise<ProjectQuickCommandsSnapshot> {
  if (result.status === 'error') {
    return EMPTY_SNAPSHOT
  }
  const commands = projectQuickCommandsForRepo(repoId, result.hooks)
  if (commands.length === 0) {
    return EMPTY_SNAPSHOT
  }
  // Why: hash the same snapshot the commands came from — a later disk edit must
  // change the hash so previously-fetched bytes cannot ride a newer approval.
  const sharedTrustContentHash = await hashOrcaHookScript(getSharedCommandTrustContent(result.hooks))
  return { commands, sharedTrustContentHash }
}

function fetchProjectQuickCommands(repoId: string): Promise<ProjectQuickCommandsSnapshot> {
  const cached = snapshotCache.get(repoId)
  if (cached) {
    return cached
  }
  const state = useAppStore.getState()
  const promise = (async (): Promise<ProjectQuickCommandsSnapshot> => {
    try {
      const result = await checkRuntimeHooks(settingsForHookRepoOwner(state, repoId), repoId)
      return await resolveProjectQuickCommandsSnapshot(repoId, result)
    } catch {
      return EMPTY_SNAPSHOT
    }
  })()
  snapshotCache.set(repoId, promise)
  return promise
}

/**
 * Re-run the shared orca.yaml trust review for a repo (same dialog and store
 * record as the setup-hook gate), then refresh the cached snapshot so menu
 * entries converge on the content the user actually approved.
 */
export async function reviewProjectQuickCommandTrust(repoId: string): Promise<'run' | 'skip'> {
  const decision = await ensureHooksConfirmed(useAppStore.getState(), repoId, 'setup')
  if (decision === 'run') {
    invalidateProjectQuickCommands(repoId)
  }
  return decision
}

function useProjectQuickCommandsGeneration(): number {
  const [generation, setGeneration] = useState(0)
  useEffect(() => {
    const listener = (): void => setGeneration((value) => value + 1)
    invalidationListeners.add(listener)
    return () => {
      invalidationListeners.delete(listener)
    }
  }, [])
  return generation
}

export type ProjectQuickCommandsState = ProjectQuickCommandsSnapshot & {
  /** True only when the repo's CURRENT trust record covers this snapshot's content hash. */
  trusted: boolean
}

export function useProjectQuickCommands(repoId: string | null): ProjectQuickCommandsState {
  const repo = useRepoById(repoId)
  const suppressed = useMemo(() => !repo || isProjectQuickCommandSourceLocalOnly(repo), [repo])
  const generation = useProjectQuickCommandsGeneration()
  const [snapshot, setSnapshot] = useState(EMPTY_SNAPSHOT)

  useEffect(() => {
    if (!repoId || suppressed) {
      setSnapshot(EMPTY_SNAPSHOT)
      return
    }
    let cancelled = false
    void fetchProjectQuickCommands(repoId).then((next) => {
      if (!cancelled) {
        setSnapshot(next)
      }
    })
    return () => {
      cancelled = true
    }
  }, [repoId, suppressed, generation])

  const trust = useAppStore((s) => (repoId ? s.trustedOrcaHooks[repoId] : undefined))
  return {
    ...snapshot,
    trusted: isSharedOrcaCommandTrusted(trust, snapshot.sharedTrustContentHash)
  }
}

/** Read-only per-repo listing for the Settings pane; repos resolving to no commands are omitted. */
export function useProjectQuickCommandsByRepo(
  repoIds: readonly string[]
): ReadonlyMap<string, TerminalQuickCommand[]> {
  const generation = useProjectQuickCommandsGeneration()
  const [byRepo, setByRepo] = useState<ReadonlyMap<string, TerminalQuickCommand[]>>(new Map())
  // Why: key on ids content, not array identity — callers rebuild the array per render.
  const idsKey = repoIds.join('\n')

  useEffect(() => {
    const ids = idsKey.length > 0 ? idsKey.split('\n') : []
    let cancelled = false
    void Promise.all(
      ids.map(async (repoId) => {
        const repo = useAppStore.getState().repos.find((candidate) => candidate.id === repoId)
        if (!repo || isProjectQuickCommandSourceLocalOnly(repo)) {
          return [repoId, [] as TerminalQuickCommand[]] as const
        }
        const snapshot = await fetchProjectQuickCommands(repoId)
        return [repoId, snapshot.commands] as const
      })
    ).then((entries) => {
      if (!cancelled) {
        setByRepo(new Map(entries.filter(([, commands]) => commands.length > 0)))
      }
    })
    return () => {
      cancelled = true
    }
  }, [idsKey, generation])

  return byRepo
}
