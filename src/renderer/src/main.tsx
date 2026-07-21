import './assets/main.css'

import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { useTranslation } from 'react-i18next'
import App from './App'
import { RecoverableRenderErrorBoundary } from './components/error-boundaries/RecoverableRenderErrorBoundary'
import {
  installRendererCrashDiagnostics,
  recordRendererCrashBreadcrumb
} from './lib/crash-diagnostics'
import { applyDocumentTheme } from './lib/document-theme'
import { startGitWasm } from './lib/git-wasm/git-line-stats'
import { startCryptoWasm } from './lib/crypto-wasm/browser-crypto-wasm'
import { shouldEnableReactGrab } from './lib/react-grab-dev-gate'
import { I18nProvider } from './i18n/I18nProvider'
import { translate } from './i18n/i18n'
import { applyAppDocumentTitle } from './startup/app-document-title'

recordRendererCrashBreadcrumb('renderer_bootstrap_started', { dev: import.meta.env.DEV })
installRendererCrashDiagnostics()
void applyAppDocumentTitle(() => window.api.app.getIdentity(), document)
// Compile the orca-git wasm eagerly. It backs the Rust agent-startup plan
// builders (session auto-resume / cold-restore run these imperatively on boot,
// with no ready-subscription), so gate the first render on it below — otherwise
// a pre-ready builder call returns null and a restored agent fails to resume.
const gitWasmReady = startGitWasm()
// Compile the E2EE crypto wasm eagerly so it is ready before any remote
// WebSocket handshake (which needs it synchronously to seal the box).
void startCryptoWasm()

if (
  import.meta.env.DEV &&
  shouldEnableReactGrab({
    dev: import.meta.env.DEV,
    enableFlag: import.meta.env.VITE_ENABLE_REACT_GRAB
  })
) {
  void import('react-grab').then(({ init }) => init())
  void import('react-grab/styles.css')
}

applyDocumentTheme('system', { disableTransitions: false })

const rootElement = document.getElementById('root')
if (!rootElement) {
  recordRendererCrashBreadcrumb('renderer_root_missing')
  throw new Error('Renderer root element not found.')
}
// Capture the narrowed element so the deferred `renderApp` closure keeps it non-null.
const rootContainer: HTMLElement = rootElement

function RendererRoot(): React.JSX.Element {
  useTranslation()
  return (
    <RecoverableRenderErrorBoundary
      boundaryId="app.root"
      surface="app-root"
      title={translate('app.recoverableError.rootTitle', 'Orca hit a renderer error.')}
      description={translate(
        'app.recoverableError.rootDescription',
        'The app shell could not finish rendering. Retry to remount it, or relaunch Orca if the error persists.'
      )}
    >
      <App />
    </RecoverableRenderErrorBoundary>
  )
}

function renderApp(): void {
  createRoot(rootContainer).render(
    <StrictMode>
      <I18nProvider>
        <RendererRoot />
      </I18nProvider>
    </StrictMode>
  )
  recordRendererCrashBreadcrumb('renderer_bootstrap_rendered')
}

// Render once the git wasm is ready so synchronous renderer helpers (agent
// startup builders, task-query parsing, etc.) never hit their pre-ready null
// fallback and then stay stuck on it (a useMemo won't recompute when readiness
// flips). A genuine compile FAILURE rejects and is caught immediately, so this
// waits for the local bundled wasm to settle; the long timeout is only an
// anti-hang backstop for a promise that never resolves (near-impossible for a
// local asset), not a routine "render without wasm" valve.
void Promise.race([
  gitWasmReady.catch(() => undefined),
  new Promise<void>((resolve) => setTimeout(resolve, 10000))
]).then(renderApp)
