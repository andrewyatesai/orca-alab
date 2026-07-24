import { app } from 'electron'
import { join } from 'node:path'

// Why: single source of truth for the plaintext cookie staging directory so the
// writer (cookie import) and the reclaimers (profile delete / undo / startup
// sweep in the session registry) can never drift onto different paths and
// orphan a decrypted cookie DB.
export const COOKIE_IMPORT_STAGING_DIR_NAME = 'cookie-import-staging'

export function cookieImportStagingDir(): string {
  return join(app.getPath('userData'), COOKIE_IMPORT_STAGING_DIR_NAME)
}
