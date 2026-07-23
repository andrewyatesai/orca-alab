import { resolve } from 'node:path'
import type { App } from 'electron'
import { ORCA_DEEP_LINK_SCHEME } from '../../shared/orca-deep-link'

// Why: dev-mode setAsDefaultProtocolClient with exec args MUTATES the OS handler
// to point at the dev Electron shim (Windows/Linux), stealing the scheme from
// the installed app — so dev registration is opt-in, never automatic.
export const DEEP_LINK_DEV_REGISTRATION_ENV = 'ORCA_DEV_REGISTER_DEEP_LINKS'

export type DeepLinkRegistrationOptions = {
  isServeMode: boolean
  /** Electron's `process.defaultApp` — true when running via the dev shim. */
  isDefaultApp: boolean
  env?: NodeJS.ProcessEnv
  execPath?: string
  /** `process.argv[1]` — the app path the dev shim was launched with. */
  appArgvPath?: string
  warn?: (message: string) => void
}

/** OS-level `orca://` handler registration. Packaged builds register plainly
 *  (electron-builder's `protocols` entry did the manifest half); serve mode
 *  registers nothing (deep links are desktop-only, design §1 non-goals). */
export function registerOrcaProtocolClient(app: App, opts: DeepLinkRegistrationOptions): boolean {
  if (opts.isServeMode) {
    return false
  }
  const warn = opts.warn ?? console.warn
  try {
    if (opts.isDefaultApp) {
      const env = opts.env ?? process.env
      if (env[DEEP_LINK_DEV_REGISTRATION_ENV] !== '1') {
        return false
      }
      const execPath = opts.execPath ?? process.execPath
      const appArgvPath = opts.appArgvPath ?? process.argv[1]
      if (!appArgvPath) {
        return false
      }
      // Why: the dev shim needs exec args so Windows/Linux invoke `electron <app> <url>`.
      return app.setAsDefaultProtocolClient(ORCA_DEEP_LINK_SCHEME, execPath, [resolve(appArgvPath)])
    }
    return app.setAsDefaultProtocolClient(ORCA_DEEP_LINK_SCHEME)
  } catch (error) {
    // Why: Linux dev/AppImage hosts can lack desktop-file integration; deep links
    // clicked INSIDE a terminal pane still work (they never hit the OS handler).
    warn(`[deep-links] orca:// scheme registration failed: ${String(error)}`)
    return false
  }
}
