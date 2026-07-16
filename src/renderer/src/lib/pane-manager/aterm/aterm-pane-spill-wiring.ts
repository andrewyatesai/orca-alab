import {
  applyAtermWindowChrome,
  readAtermEffectsConfig,
  type AtermEffectsTarget
} from './aterm-effects-settings'
import { createAtermSpillBlit, hasAtermSpillExports } from './aterm-spill-blit'
import { atermSpillOverlay } from './aterm-spill-overlay'
import type { AtermTerminal } from './aterm_wasm.js'

// Cross-pane spill wiring for one pane. IN-PROCESS (stage 3): marks a REAL
// engine that exports the spill surface as capable — flipping stage 2's
// fail-closed registration seam live — and owns the per-paint blit plus the
// late paneKey bind and teardown. WORKER path (stage 4): the loader already
// marked the facade (from the worker's STATE capability echo) and attached its
// spill message channel; this wiring only owns the paneKey bind + teardown —
// compositing runs worker-side, so there is no main-thread blit.

export type AtermPaneSpillWiring = {
  /** Presenter dep: the per-paint spill pass; undefined when not capable OR on
   *  the worker path (its compositor rides the worker frame scheduler). */
  spillBlit: (() => void) | undefined
  /** Controller seam: late-bind the durable overlay identity (attach edge). */
  bindSpillPaneKey: (paneKey: string) => void
  /** Teardown: drop this pane's overlay strips with the engine. */
  unregister: () => void
}

/** `memory` is the engine module's linear memory (undefined on the worker
 *  path); `shared` carries the overlay key across context-loss rebuilds so the
 *  replacement engine re-registers under the same identity. */
export function wireAtermPaneSpill(
  term: AtermTerminal,
  memory: WebAssembly.Memory | undefined,
  shared: { spillPaneKey: string | null },
  scheduleDraw: () => void,
  isDisposed: () => boolean
): AtermPaneSpillWiring {
  const spillTarget = term as typeof term &
    Pick<AtermEffectsTarget, 'spillExportCapable' | 'spillPaneKey'>
  const inProcessCapable = memory !== undefined && hasAtermSpillExports(term)
  if (inProcessCapable) {
    spillTarget.spillExportCapable = true
  }
  // Worker facade terms arrive pre-marked by the loader (STATE echo); either
  // capability kind binds/unregisters identically — only the blit differs.
  const capable = inProcessCapable || spillTarget.spillExportCapable === true
  if (capable && shared.spillPaneKey !== null) {
    // A context-loss rebuild already knows its overlay identity; fresh panes get
    // theirs at the controller-attach edge via bindSpillPaneKey.
    spillTarget.spillPaneKey = shared.spillPaneKey
  }
  return {
    spillBlit:
      inProcessCapable && memory !== undefined
        ? createAtermSpillBlit({
            term,
            memory,
            getPaneKey: () => spillTarget.spillPaneKey,
            isDisposed,
            scheduleDraw
          })
        : undefined,
    // The tab id is only resolvable at the controller-attach edge; the chrome
    // re-derive lets a glow granted BEFORE the key existed register now.
    bindSpillPaneKey: (paneKey: string): void => {
      shared.spillPaneKey = paneKey
      if (!capable || isDisposed()) {
        return
      }
      spillTarget.spillPaneKey = paneKey
      applyAtermWindowChrome(spillTarget, readAtermEffectsConfig())
    },
    unregister: (): void => {
      if (typeof spillTarget.spillPaneKey === 'string' && spillTarget.spillPaneKey.length > 0) {
        atermSpillOverlay.unregister(spillTarget.spillPaneKey)
      }
    }
  }
}
