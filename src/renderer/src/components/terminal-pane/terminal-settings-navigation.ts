// The context menu's "Terminal Settings…" jump (#9279 / CM-A5): deep-link into
// the Terminal settings pane scoped to the pane's repo. There is no pane-scoped
// settings model, so pane:'terminal' + the repo id is the deepest existing target.

import { useAppStore } from '@/store'

export function openTerminalSettingsForRepo(repoId: string | null): void {
  const store = useAppStore.getState()
  store.openSettingsTarget({ pane: 'terminal', repoId })
  store.openSettingsPage()
}
