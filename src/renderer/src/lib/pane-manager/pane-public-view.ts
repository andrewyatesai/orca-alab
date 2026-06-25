import type { ManagedPane, ManagedPaneInternal } from './pane-manager-types'

export function toPublicPane(pane: ManagedPaneInternal): ManagedPane {
  const view: ManagedPane = {
    id: pane.id,
    leafId: pane.leafId,
    stablePaneId: pane.stablePaneId,
    terminal: pane.terminal,
    container: pane.container,
    linkTooltip: pane.linkTooltip,
    fitAddon: pane.fitAddon,
    searchAddon: pane.searchAddon,
    serializeAddon: pane.serializeAddon,
    atermController: pane.atermController ?? null
  }
  // Why accessors (not copies): connectPanePty installs routePtyInput/route
  // PtyResize on the pane it's handed (a fresh public view each call), but the
  // aterm controller's input/resize sinks read them off the INTERNAL pane that
  // openAtermPane captured. Back both by the internal pane so a write through any
  // view reaches the sink — otherwise aterm input + drained query replies are
  // silently dropped (routePtyInput stays undefined on the captured pane).
  Object.defineProperty(view, 'routePtyInput', {
    enumerable: true,
    configurable: true,
    get: () => pane.routePtyInput,
    set: (value: ManagedPane['routePtyInput']) => {
      pane.routePtyInput = value
    }
  })
  Object.defineProperty(view, 'routePtyResize', {
    enumerable: true,
    configurable: true,
    get: () => pane.routePtyResize,
    set: (value: ManagedPane['routePtyResize']) => {
      pane.routePtyResize = value
    }
  })
  return view
}
