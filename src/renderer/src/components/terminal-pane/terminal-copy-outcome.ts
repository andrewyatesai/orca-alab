import { toast } from 'sonner'
import { useAppStore } from '@/store'
import { OSC52_CLIPBOARD_SETTING_ID } from './osc52-clipboard-setting-anchor'
import { translate } from '@/i18n/i18n'

// The single renderer seam for terminal clipboard-copy outcomes (PC-5611/8977):
// every copy path (shortcut, context menu, copy-on-select, OSC 52) routes its
// verified main-process result here so failures stop being invisible. Success
// stays silent for host-initiated copies (the user performed them); the ONLY
// success signal is a one-time passive toast for the first OSC 52 write, since a
// TUI-initiated copy is otherwise indistinguishable from the gated/silent-failure
// cases the issues describe.

export type TerminalCopySource = 'shortcut' | 'context-menu' | 'copy-on-select' | 'osc52'

// Per-session, per-source failure latch (same pattern as the OSC 52 blocked
// toast) — it also rate-limits copy-on-select, which can fire per drag.
const failureToastShownForSource = new Set<TerminalCopySource>()
let osc52SuccessToastShown = false

/** Test-only: clear the per-session toast latches. */
export function resetTerminalCopyOutcomeLatchesForTest(): void {
  failureToastShownForSource.clear()
  osc52SuccessToastShown = false
}

function openOsc52Setting(): void {
  const store = useAppStore.getState()
  // Why: open the exact row so the failure points at the OSC 52 gate it names.
  store.setSettingsSearchQuery('')
  store.openSettingsTarget({
    pane: 'terminal',
    repoId: null,
    sectionId: OSC52_CLIPBOARD_SETTING_ID
  })
  store.openSettingsPage()
}

export function reportTerminalCopyOutcome(ok: boolean, source: TerminalCopySource): void {
  if (ok) {
    if (source === 'osc52' && !osc52SuccessToastShown) {
      osc52SuccessToastShown = true
      toast.success(
        translate(
          'auto.components.terminal.pane.terminal.copy.outcome.osc52Success',
          'Terminal copied to clipboard'
        ),
        {
          description: translate(
            'auto.components.terminal.pane.terminal.copy.outcome.osc52SuccessDescription',
            'A terminal program wrote your clipboard via OSC 52. Later copies stay silent.'
          )
        }
      )
    }
    return
  }
  if (failureToastShownForSource.has(source)) {
    return
  }
  failureToastShownForSource.add(source)
  toast.error(
    translate(
      'auto.components.terminal.pane.terminal.copy.outcome.failureTitle',
      'Copy failed — clipboard unchanged'
    ),
    {
      description: translate(
        'auto.components.terminal.pane.terminal.copy.outcome.failureDescription',
        'The clipboard write could not be verified. Try copying again.'
      ),
      duration: 10_000,
      action:
        source === 'osc52'
          ? {
              label: translate(
                'auto.components.terminal.pane.terminal.copy.outcome.openSetting',
                'Open Setting'
              ),
              onClick: openOsc52Setting
            }
          : undefined
    }
  )
}

/** Write `text` via the verified main-process seam and surface the outcome for
 *  `source`. Resolves with the verified result (false = clipboard unchanged). */
export function copyTerminalTextVerified(
  text: string,
  source: TerminalCopySource
): Promise<boolean> {
  // Why: e2e/hidden windows can lack the IPC surface entirely — nothing was
  // attempted, so stay silent (the pre-verification behavior) instead of toasting.
  const write = window.api?.ui?.writeClipboardText
  if (!write) {
    return Promise.resolve(false)
  }
  return (
    write(text)
      // Why: a legacy/void resolution must not read as failure — only an explicit false does.
      .then((ok) => ok !== false)
      .catch(() => false)
      .then((ok) => {
        reportTerminalCopyOutcome(ok, source)
        return ok
      })
  )
}

/** Context-menu right-click copy: clear the selection ONLY after the write
 *  verified — clearing first destroyed the selection even when the copy never
 *  landed (the issue's exact complaint). */
export async function copyTerminalSelectionThenClear(
  text: string,
  clearSelection: (() => void) | null
): Promise<boolean> {
  const ok = await copyTerminalTextVerified(text, 'context-menu')
  if (ok) {
    clearSelection?.()
  }
  return ok
}
