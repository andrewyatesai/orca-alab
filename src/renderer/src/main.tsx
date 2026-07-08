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

recordRendererCrashBreadcrumb('renderer_bootstrap_started', { dev: import.meta.env.DEV })
installRendererCrashDiagnostics()
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

// Render once the git wasm is ready so the agent-startup builders never hit
// their pre-ready null fallback. The wasm is a local bundled asset (~tens of ms
// to compile); the timeout is a safety valve so a stalled/failed compile still
// renders the shell — line stats then degrade to numstat until it recovers.
void Promise.race([
  gitWasmReady.catch(() => undefined),
  new Promise<void>((resolve) => setTimeout(resolve, 2000))
]).then(renderApp)
