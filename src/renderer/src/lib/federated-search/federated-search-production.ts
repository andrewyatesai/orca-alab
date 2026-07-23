// Production wiring for the federated palette: the controller with the live
// local-pane adapter (worker fan-out + in-process fallback) and the jump deps
// (store activation, registry pane resolution, parked re-run-nearest). The
// daemon/parked/remote adapters plug into the SAME controller seam when their
// tracks (5B/5C) land — the dedup plan and merge contracts here already handle
// their batches.

import { toast } from 'sonner'
import { useAppStore } from '@/store'
import {
  getLivePaneManagersForTab,
  getRegisteredTabPaneManagerTabIds
} from '../pane-manager/pane-manager-registry'
import {
  postAtermFederatedToSharedWorker,
  subscribeAtermSharedWorkerFederatedEvents
} from '../pane-manager/aterm/aterm-shared-render-worker'
import { runFederatedPaneScan } from '../pane-manager/aterm/aterm-federated-budgeted-scan'
import type { AtermFederatedSearchTarget } from '../pane-manager/aterm/aterm-federated-search-target'
import { translate } from '@/i18n/i18n'
import {
  createFederatedSearchController,
  type FederatedSearchController
} from './federated-search-controller'
import { createLivePaneSearchAdapter } from './live-pane-search-adapter'
import { createRemotePaneSearchAdapter } from './remote-pane-search-adapter'
import { discoverLiveFederatedPanes } from './live-pane-discovery'
import {
  discoverRemoteFederatedPanes,
  productionRemoteSearchCall
} from './remote-pane-discovery'
import type { FederatedGroupOrderContext } from './federated-search-grouping'
import type { FederatedPaneRef, FederatedQueryOpts } from './federated-search-model'
import {
  jumpToFederatedResult,
  type FederatedJumpDeps,
  type FederatedJumpOutcome,
  type FederatedJumpPane
} from './federated-search-navigation'
import type { FederatedResultGroup } from './federated-search-grouping'
import type { FederatedMatch } from './federated-search-model'

// Un-park wait: activation mounts the pane; the engine restore (snapshot/replay)
// finishes within a few frames on healthy hosts — poll briefly, bounded.
const UNPARK_POLL_INTERVAL_MS = 120
const UNPARK_TIMEOUT_MS = 8000
// One-off single-pane re-runs must never collide with palette generations (the
// worker keeps one active run keyed by gen; palette gens are small integers).
let nextRerunGen = 1_000_000

type RegistryPaneView = {
  leafId: string
  atermController?: {
    federatedSearchTarget?: () => unknown
    scrollToLine?: (line: number) => void
    textarea?: HTMLTextAreaElement
  } | null
}

function findRegistryPane(paneRef: FederatedPaneRef): RegistryPaneView | null {
  for (const tabId of getRegisteredTabPaneManagerTabIds()) {
    if (tabId !== paneRef.tabId) {
      continue
    }
    for (const manager of getLivePaneManagersForTab(tabId)) {
      try {
        const pane = (manager.getPanes() as RegistryPaneView[]).find(
          (p) => p.leafId === paneRef.leafId
        )
        if (pane) {
          return pane
        }
      } catch {
        // Manager tearing down — inspect siblings.
      }
    }
  }
  return null
}

function toJumpPane(pane: RegistryPaneView): FederatedJumpPane | null {
  const controller = pane.atermController
  if (!controller || typeof controller.scrollToLine !== 'function') {
    return null
  }
  return {
    scrollToLine: (absRow) => controller.scrollToLine!(absRow),
    focus: () => controller.textarea?.focus()
  }
}

/** Single-pane, non-perturbing re-run for the parked-jump contract. */
async function rerunQueryInPane(
  paneRef: FederatedPaneRef,
  query: string,
  opts: FederatedQueryOpts
): Promise<{ absRow: number }[] | null> {
  const pane = findRegistryPane(paneRef)
  const target = pane?.atermController?.federatedSearchTarget?.() as
    | AtermFederatedSearchTarget
    | null
    | undefined
  if (!target) {
    return null
  }
  if (target.kind === 'in-process') {
    return new Promise((resolve) => {
      runFederatedPaneScan({
        engine: target.engine,
        query,
        caseSensitive: opts.caseSensitive,
        isRegex: opts.isRegex,
        maxMatches: 1000,
        isCancelled: () => false,
        yieldSlice: (next) => void setTimeout(next, 0),
        onDone: (result) => resolve(result ? result.matches : null)
      })
    })
  }
  return new Promise((resolve) => {
    const gen = nextRerunGen++
    const rows: { absRow: number }[] = []
    let unsubscribe: () => void = () => undefined
    const timer = setTimeout(() => {
      unsubscribe()
      resolve(null)
    }, UNPARK_TIMEOUT_MS)
    unsubscribe = subscribeAtermSharedWorkerFederatedEvents((event) => {
      if (event.gen !== gen) {
        return
      }
      if (event.type === 'federatedBatch') {
        for (const m of event.matches) {
          rows.push({ absRow: m.absRow })
        }
        return
      }
      clearTimeout(timer)
      unsubscribe()
      resolve(rows)
    })
    const posted = postAtermFederatedToSharedWorker({
      type: 'federatedFind',
      gen,
      query,
      caseSensitive: opts.caseSensitive,
      isRegex: opts.isRegex,
      maxPerPane: 1000,
      panes: [{ paneId: target.workerPaneId, visible: true }]
    })
    if (!posted) {
      clearTimeout(timer)
      unsubscribe()
      resolve(null)
    }
  })
}

function productionJumpDeps(): FederatedJumpDeps {
  return {
    resolvePane: (paneRef) => {
      const pane = findRegistryPane(paneRef)
      return pane ? toJumpPane(pane) : null
    },
    activatePane: (paneRef) => {
      const store = useAppStore.getState()
      if (paneRef.worktreeId !== null && store.activeWorktreeId !== paneRef.worktreeId) {
        store.setActiveWorktree(paneRef.worktreeId)
      }
      store.setActiveTab(paneRef.tabId)
    },
    waitForPane: async (paneRef) => {
      const deadline = Date.now() + UNPARK_TIMEOUT_MS
      while (Date.now() < deadline) {
        const pane = findRegistryPane(paneRef)
        const jump = pane ? toJumpPane(pane) : null
        if (jump) {
          return jump
        }
        await new Promise((r) => setTimeout(r, UNPARK_POLL_INTERVAL_MS))
      }
      return null
    },
    rerunQueryInPane,
    // Daemon inline expansion arrives with the daemon adapter track (5C); until
    // then a stale paneRef degrades to the honest "pane no longer available".
    expandDaemonInline: () => false,
    notifyJumpOutcome: (kind) => {
      if (kind === 'missing-match') {
        toast.info(
          translate(
            'auto.lib.federatedSearch.missingMatch',
            'The restored scrollback no longer contains this match.'
          )
        )
      } else {
        toast.info(
          translate('auto.lib.federatedSearch.paneUnavailable', 'Pane no longer available.')
        )
      }
    }
  }
}

/** Jump entry the palette calls on result selection. */
export function jumpToFederatedResultInApp(
  group: Pick<FederatedResultGroup, 'paneRef' | 'sessionId'>,
  match: FederatedMatch,
  query: string,
  opts: FederatedQueryOpts
): Promise<FederatedJumpOutcome> {
  return jumpToFederatedResult(group, match, query, opts, productionJumpDeps())
}

function productionOrderContext(): FederatedGroupOrderContext {
  const panes = discoverLiveFederatedPanes()
  const focused = panes.find((p) => p.focused)
  return {
    focusedPaneKey: focused?.paneRef.paneKey ?? null,
    visiblePaneKeys: new Set(panes.filter((p) => p.visible).map((p) => p.paneRef.paneKey)),
    outputRecency: (paneKey) => panes.find((p) => p.paneRef.paneKey === paneKey)?.lastOutputAt ?? 0
  }
}

/** Build the production controller (live/hidden local panes; further sources
 *  attach here as their tracks land). One instance per palette mount. */
export function createProductionFederatedSearchController(): FederatedSearchController {
  return createFederatedSearchController({
    adapters: [
      createLivePaneSearchAdapter({
        discoverPanes: discoverLiveFederatedPanes,
        postFederated: postAtermFederatedToSharedWorker,
        subscribeFederated: subscribeAtermSharedWorkerFederatedEvents
      }),
      // Remote/SSH source over the landed 5B wire (§2.4). Streams in behind the
      // local results; enumerates live remote panes from the transport-populated
      // registry (see remote-pane-discovery). Its host-row → client-row remap and
      // same-session dedup already compose with the controller's merge.
      createRemotePaneSearchAdapter({
        discoverRemotePanes: discoverRemoteFederatedPanes,
        searchRemote: productionRemoteSearchCall
      })
    ],
    orderContext: productionOrderContext
  })
}
