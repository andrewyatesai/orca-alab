import { app } from 'electron'
import { join } from 'node:path'
import { getOrcaProfileBrowserPartitionSegment } from '../../shared/orca-profiles'

// Why: single source of truth for the plaintext cookie staging directory so the
// writer (cookie import) and the reclaimers (profile delete / undo / startup
// sweep in the session registry) can never drift onto different paths and
// orphan a decrypted cookie DB.
export const COOKIE_IMPORT_STAGING_DIR_NAME = 'cookie-import-staging'

// Why: the containment root for tamper-guarding staged-path unlinks. Every
// per-profile staging subdir lives under this, so a resolved path outside it is
// never a legitimate staged DB.
export function cookieImportStagingRoot(): string {
  return join(app.getPath('userData'), COOKIE_IMPORT_STAGING_DIR_NAME)
}

// Why: userData (and thus the staging root) is shared across Orca profiles, but
// pendingCookieImports — and the startup sweep that reclaims orphans against it —
// are per-Orca-profile. Namespace each profile's staged DBs into its own subdir
// so one profile's sweep can only ever enumerate/delete its own staged files and
// never another profile's still-pending decrypted cookie DB (cxb3).
export function orcaProfileCookieStagingDir(orcaProfileId: string): string {
  return join(cookieImportStagingRoot(), getOrcaProfileBrowserPartitionSegment(orcaProfileId))
}
