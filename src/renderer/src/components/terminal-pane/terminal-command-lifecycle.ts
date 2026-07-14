import type { IDisposable } from '../../lib/pane-manager/aterm/terminal-types'
import type { AtermTerminalFacade as Terminal } from '@/lib/pane-manager/aterm/aterm-terminal-facade'
import { createOsc133CommandFinishedScanner } from '../../../../shared/terminal-osc133-command-finished'

type TerminalCommandLifecycleOptions = {
  onCommandFinished: (bestEffortExitCode: number | null) => void
  /** OSC 133;C — the shell is about to execute the entered command; the pane's foreground is changing. */
  onCommandStarted?: () => void
}

export function createTerminalCommandLifecycle(options: TerminalCommandLifecycleOptions): {
  handlePtyData: (data: string) => void
  attachXtermConsumer: (terminal: Terminal) => IDisposable
  dispose: () => void
} {
  // Why: the byte parsing lives in shared so main's side-effect tracker emits
  // identical command-finished facts for local/SSH PTYs; this renderer wrapper
  // remains the byte path for remote-runtime PTYs and the kill-switch-off mode.
  const scanner = createOsc133CommandFinishedScanner(
    options.onCommandFinished,
    options.onCommandStarted
  )
  const disposables: IDisposable[] = []

  return {
    handlePtyData: scanner.scan,
    attachXtermConsumer(terminal) {
      // Why: swallow OSC 133 so shell-integration markers never paint —
      // rendering hygiene that applies regardless of side-effect authority.
      const disposable = terminal.parser.registerOscHandler(133, () => true)
      disposables.push(disposable)
      return disposable
    },
    dispose() {
      scanner.reset()
      for (const disposable of disposables.splice(0)) {
        disposable.dispose()
      }
    }
  }
}
