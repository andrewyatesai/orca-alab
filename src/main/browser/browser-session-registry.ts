/* eslint-disable max-lines -- Why: single source of truth for browser session profiles, partition allowlisting, cookie staging, and per-partition policies; splitting scatters the security boundary. */
import { app, session } from 'electron'
import type { Session } from 'electron'
import { randomUUID } from 'node:crypto'
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  unlinkSync,
  writeFileSync
} from 'node:fs'
import { dirname, join, resolve, sep } from 'node:path'
import { ORCA_BROWSER_PARTITION } from '../../shared/constants'
import {
  DEFAULT_LOCAL_ORCA_PROFILE_ID,
  getOrcaProfileBrowserDefaultPartition,
  getOrcaProfileBrowserPartitionSegment,
  getOrcaProfileBrowserSessionPartition
} from '../../shared/orca-profiles'
import type { BrowserSessionProfile, BrowserSessionProfileScope } from '../../shared/types'
import { browserManager } from './browser-manager'
import { hasSystemMediaAccess, requestSystemMediaAccess } from './browser-media-access'
import { cleanElectronUserAgent, setupClientHintsOverride } from './browser-session-ua'
import { resolveChromiumCookiesPath } from './chromium-cookie-path'
import { isBrowserSessionPermissionAllowed } from './browser-session-permission-policy'
import { cookieImportStagingRoot, orcaProfileCookieStagingDir } from './cookie-import-staging-path'
import {
  allowsBrowserWebAuthnPermission,
  clearBrowserWebAuthnAccessHandlers,
  installBrowserWebAuthnAccessHandlers
} from './browser-webauthn-access'

type BrowserSessionMeta = {
  defaultSource: BrowserSessionProfile['source']
  userAgent: string | null
  userAgentByPartition: Record<string, string>
  pendingCookieDbPath: string | null
  pendingCookieImports: Record<string, string>
  profiles: BrowserSessionProfile[]
}

export type BrowserSessionRegistryProfileOptions = {
  orcaProfileId: string
  profileDirectory: string
}

const BROWSER_SESSION_META_FILE_NAME = 'browser-session-meta.json'
const LEGACY_BROWSER_SESSION_PARTITION_RE =
  /^persist:orca-browser-session-[\da-f-]{8}-[\da-f-]{4}-[\da-f-]{4}-[\da-f-]{4}-[\da-f-]{12}$/

// Why: source of truth for valid partitions; will-attach-webview consults it so a compromised renderer can't smuggle in an arbitrary partition.

class BrowserSessionRegistry {
  private readonly profiles = new Map<string, BrowserSessionProfile>()
  private activeOrcaProfileId = DEFAULT_LOCAL_ORCA_PROFILE_ID
  private metadataPathOverride: string | null = null
  private defaultPartition = ORCA_BROWSER_PARTITION
  // Why: media (camera/mic) must be granted per requesting origin, not blanket
  // per session — an app-level OS grant is not consent for every page.
  private readonly grantedMediaOriginsByPartition = new Map<string, Set<string>>()

  constructor() {
    this.resetDefaultProfile()
  }

  configureForOrcaProfile(options: BrowserSessionRegistryProfileOptions): void {
    this.activeOrcaProfileId = options.orcaProfileId
    this.metadataPathOverride = join(options.profileDirectory, BROWSER_SESSION_META_FILE_NAME)
    this.defaultPartition = getOrcaProfileBrowserDefaultPartition(options.orcaProfileId)
    this.profiles.clear()
    this.resetDefaultProfile()
  }

  private resetDefaultProfile(): void {
    const persisted = this.loadPersistedSource()
    this.profiles.set('default', {
      id: 'default',
      scope: 'default',
      partition: this.defaultPartition,
      label: 'Default',
      source: persisted
    })
  }

  // Why: source metadata must persist across restarts (for the Settings import status) since the registry is in-memory only.
  private get metadataPath(): string {
    return (
      this.metadataPathOverride ?? join(app.getPath('userData'), BROWSER_SESSION_META_FILE_NAME)
    )
  }

  private loadPersistedSource(): BrowserSessionProfile['source'] {
    return this.loadPersistedMeta().defaultSource
  }

  private static partitionCookiesPath(partition: string): string {
    const partitionName = partition.replace('persist:', '')
    const partitionDir = join(app.getPath('userData'), 'Partitions', partitionName)
    // Why: replay must overwrite the same (modern or legacy) DB the importing partition already uses.
    return resolveChromiumCookiesPath(partitionDir) ?? join(partitionDir, 'Cookies')
  }

  // Why: the staged cookie DB holds DECRYPTED cookies; reclaim it (plus WAL/SHM
  // sidecars) when an import is undone/deleted so plaintext session cookies can't
  // linger on disk. Containment-guard the path so tampered metadata can't point
  // the unlink at an arbitrary file outside the staging dir.
  private unlinkStagedCookieDb(stagedPath: string | undefined | null): void {
    if (!stagedPath) {
      return
    }
    let stagingRoot: string
    try {
      stagingRoot = resolve(cookieImportStagingRoot())
    } catch {
      return
    }
    const resolved = resolve(stagedPath)
    if (resolved !== stagingRoot && !resolved.startsWith(stagingRoot + sep)) {
      return
    }
    for (const suffix of ['', '-wal', '-shm']) {
      try {
        unlinkSync(`${resolved}${suffix}`)
      } catch {
        /* best-effort — file may not exist */
      }
    }
  }

  // Why: the staging root is shared across Orca profiles but pendingCookieImports
  // is per-profile; scope the sweep (and the writer, via getCookieStagingDir) to
  // the ACTIVE profile's subdir so it can never delete another profile's staged DB.
  private cookieStagingDir(): string {
    return orcaProfileCookieStagingDir(this.activeOrcaProfileId)
  }

  // Why: the cookie-import writer must stage into the same per-profile subdir the
  // sweep reclaims from, or staged files would be orphaned (writer) or never
  // cleaned (sweep). Single source of truth for the active profile's staging dir.
  getCookieStagingDir(): string {
    return this.cookieStagingDir()
  }

  // Why: crash-orphaned staged DBs (import interrupted before the metadata entry
  // was cleared, or after it was dropped) would otherwise accumulate decrypted
  // cookies forever; sweep any staged file no live import still references.
  private sweepOrphanedCookieStaging(referenced: Set<string>): void {
    let stagingRoot: string
    try {
      stagingRoot = resolve(this.cookieStagingDir())
    } catch {
      return
    }
    // Why: fail-closed — a non-string/malformed reference (tampered meta) must not
    // throw out of app init; skip it rather than resolve() it. See loadPersistedMeta.
    const keep = new Set<string>()
    for (const path of referenced) {
      try {
        keep.add(resolve(path as string))
      } catch {
        /* skip malformed reference */
      }
    }
    let entries: string[]
    try {
      entries = readdirSync(stagingRoot)
    } catch {
      return // dir may not exist yet
    }
    for (const entry of entries) {
      if (!entry.startsWith('Cookies-')) {
        continue
      }
      const full = join(stagingRoot, entry)
      const base = full.replace(/-(wal|shm)$/, '')
      if (keep.has(base)) {
        continue
      }
      try {
        unlinkSync(full)
      } catch {
        /* best-effort */
      }
    }
  }

  // Why: write-temp-then-rename is atomic, so a crash mid-write can't corrupt the live file.
  private persistMeta(updates: Partial<BrowserSessionMeta>): void {
    try {
      const existing = this.loadPersistedMeta()
      const tmpPath = `${this.metadataPath}.tmp`
      mkdirSync(dirname(this.metadataPath), { recursive: true })
      writeFileSync(tmpPath, JSON.stringify({ ...existing, ...updates }))
      renameSync(tmpPath, this.metadataPath)
    } catch {
      // best-effort
    }
  }

  private persistSource(source: BrowserSessionProfile['source'], userAgent?: string | null): void {
    this.persistMeta({
      defaultSource: source,
      ...(userAgent !== undefined ? { userAgent } : {})
    })
  }

  // Why: non-default profiles are in-memory only; without this they vanish on restart.
  private persistProfiles(): void {
    const nonDefault = [...this.profiles.values()].filter((p) => p.id !== 'default')
    this.persistMeta({ profiles: nonDefault })
  }

  private loadPersistedMeta(): BrowserSessionMeta {
    try {
      const raw = readFileSync(this.metadataPath, 'utf-8')
      const data = JSON.parse(raw)
      const legacyUserAgent = typeof data?.userAgent === 'string' ? data.userAgent : null
      const userAgentByPartition: Record<string, string> =
        data && typeof data.userAgentByPartition === 'object' && data.userAgentByPartition
          ? { ...data.userAgentByPartition }
          : {}
      if (legacyUserAgent && !userAgentByPartition[this.defaultPartition]) {
        userAgentByPartition[this.defaultPartition] = legacyUserAgent
      }

      const legacyPendingCookieDbPath =
        typeof data?.pendingCookieDbPath === 'string' ? data.pendingCookieDbPath : null
      // Why: fail-closed — a well-formed JSON meta whose pendingCookieImports holds
      // a NON-STRING value (tampered/corrupt) reaches the startup sweep, where
      // resolve()/path ops would throw OUTSIDE any try/catch during app init and
      // brick launch. Drop non-string values so only real staged paths survive.
      const pendingCookieImports: Record<string, string> = {}
      if (data && typeof data.pendingCookieImports === 'object' && data.pendingCookieImports) {
        for (const [partition, stagedPath] of Object.entries(data.pendingCookieImports)) {
          if (typeof stagedPath === 'string') {
            pendingCookieImports[partition] = stagedPath
          }
        }
      }
      if (legacyPendingCookieDbPath && !pendingCookieImports[this.defaultPartition]) {
        pendingCookieImports[this.defaultPartition] = legacyPendingCookieDbPath
      }
      return {
        defaultSource: data?.defaultSource ?? null,
        userAgent: legacyUserAgent,
        userAgentByPartition,
        pendingCookieDbPath: legacyPendingCookieDbPath,
        pendingCookieImports,
        profiles: Array.isArray(data?.profiles) ? data.profiles : []
      }
    } catch {
      return {
        defaultSource: null,
        userAgent: null,
        userAgentByPartition: {},
        pendingCookieDbPath: null,
        pendingCookieImports: {},
        profiles: []
      }
    }
  }

  // Why: run before any webview loads, and set the UA before the first request or Electron's default UA invalidates imported cookies.
  // Why re-read defaultSource: the constructor may run before app.isReady() (userData path unavailable), so loadPersistedSource() returned null.
  initializeBrowserSessionsFromPersistedState(): void {
    const meta = this.loadPersistedMeta()
    // Why: reclaim any staged plaintext cookie DB no live import references,
    // including crash-orphans that predate this metadata snapshot.
    this.sweepOrphanedCookieStaging(new Set(Object.values(meta.pendingCookieImports)))
    if (meta.defaultSource) {
      const current = this.profiles.get('default')
      if (current && current.source === null) {
        this.profiles.set('default', { ...current, source: meta.defaultSource })
      }
    }
    if (meta.profiles.length > 0) {
      this.hydrateFromPersisted(meta.profiles)
    }

    // Why: nothing else installs policies on the default partition (hydrate skips it), so without this its guest permissions would be denied.
    this.setupSessionPolicies(this.defaultPartition)

    const partitions = new Set([
      this.defaultPartition,
      ...this.listProfiles().map((p) => p.partition)
    ])
    for (const partition of partitions) {
      try {
        const sess = session.fromPartition(partition)
        const persistedUa = meta.userAgentByPartition[partition]
        if (persistedUa) {
          sess.setUserAgent(persistedUa)
          setupClientHintsOverride(sess, persistedUa)
          continue
        }

        // Why: the default Electron UA leaks "Electron/X.X.X" + app name, which trips Cloudflare Turnstile.
        const cleanUA = cleanElectronUserAgent(sess.getUserAgent())
        sess.setUserAgent(cleanUA)
        setupClientHintsOverride(sess, cleanUA)
      } catch {
        /* session not available yet (e.g. unit tests or pre-ready) */
      }
    }
  }

  // Why: must run before any session.fromPartition() so CookieMonster reads the staged cookies instead of overwriting them from its in-memory DB.
  applyPendingCookieImport(): void {
    try {
      const meta = this.loadPersistedMeta()
      const pendingEntries = Object.entries(meta.pendingCookieImports)
      if (pendingEntries.length === 0) {
        return
      }
      // Why: replay writes to partition-derived paths, so corrupted metadata must pass the same validation as the webview allowlist.
      const knownPartitions = new Set([this.defaultPartition])
      for (const profile of meta.profiles) {
        if (this.isValidPersistedProfile(profile)) {
          knownPartitions.add(profile.partition)
        }
      }
      const remainingEntries = { ...meta.pendingCookieImports }

      for (const [partition, stagedPath] of pendingEntries) {
        if (!knownPartitions.has(partition)) {
          // Why: dropping the entry for an unknown/invalid partition must also
          // unlink the plaintext staged DB, or it becomes an unreachable orphan.
          this.unlinkStagedCookieDb(stagedPath)
          delete remainingEntries[partition]
          continue
        }
        if (!existsSync(stagedPath)) {
          delete remainingEntries[partition]
          continue
        }

        const liveCookiesPath = BrowserSessionRegistry.partitionCookiesPath(partition)
        try {
          mkdirSync(join(liveCookiesPath, '..'), { recursive: true })
          copyFileSync(stagedPath, liveCookiesPath)
          // Why: stale WAL/SHM sidecars would corrupt CookieMonster's read of the freshly swapped DB.
          let sidecarCopyFailed = false
          for (const suffix of ['-wal', '-shm']) {
            try {
              unlinkSync(liveCookiesPath + suffix)
            } catch {
              /* may not exist */
            }
            const stagingSidecar = stagedPath + suffix
            if (!existsSync(stagingSidecar)) {
              continue
            }
            try {
              copyFileSync(stagingSidecar, liveCookiesPath + suffix)
            } catch {
              sidecarCopyFailed = true
            }
          }
          if (sidecarCopyFailed) {
            // Why: sidecar copy failed → inconsistent replay; keep this entry for retry.
            continue
          }
          for (const ext of ['', '-wal', '-shm']) {
            try {
              unlinkSync(`${stagedPath}${ext}`)
            } catch {
              /* best-effort */
            }
          }
          delete remainingEntries[partition]
        } catch {
          // Why: keep this entry for retry — one partition's failed replay shouldn't drop unrelated entries.
        }
      }
      this.persistMeta({
        pendingCookieImports: remainingEntries,
        pendingCookieDbPath: remainingEntries[this.defaultPartition] ?? null
      })
    } catch {
      // best-effort — if this fails, CookieMonster loads the old DB
    }
  }

  setPendingCookieImport(partition: string, stagingDbPath: string): void {
    const meta = this.loadPersistedMeta()
    const pendingCookieImports = { ...meta.pendingCookieImports, [partition]: stagingDbPath }
    this.persistMeta({
      pendingCookieImports,
      pendingCookieDbPath: pendingCookieImports[this.defaultPartition] ?? null
    })
  }

  persistUserAgent(partition: string, userAgent: string | null): void {
    const meta = this.loadPersistedMeta()
    const userAgentByPartition = { ...meta.userAgentByPartition }
    if (userAgent) {
      userAgentByPartition[partition] = userAgent
    } else {
      delete userAgentByPartition[partition]
    }
    this.persistMeta({
      userAgentByPartition,
      userAgent: userAgentByPartition[this.defaultPartition] ?? null
    })
  }

  getDefaultProfile(): BrowserSessionProfile {
    return this.profiles.get('default')!
  }

  getProfile(profileId: string): BrowserSessionProfile | null {
    return this.profiles.get(profileId) ?? null
  }

  listProfiles(): BrowserSessionProfile[] {
    return [...this.profiles.values()]
  }

  isAllowedPartition(partition: string): boolean {
    if (partition === this.defaultPartition) {
      return true
    }
    return [...this.profiles.values()].some((p) => p.partition === partition)
  }

  resolvePartition(profileId: string | null | undefined): string {
    if (!profileId) {
      return this.defaultPartition
    }
    return this.profiles.get(profileId)?.partition ?? this.defaultPartition
  }

  resolveKnownPartition(profileId: string | null | undefined): string | null {
    if (!profileId) {
      // Why: use the active Orca profile's default partition, not the legacy constant, or profiles resolve local-default's cookie jar.
      return this.defaultPartition
    }
    return this.profiles.get(profileId)?.partition ?? null
  }

  createProfile(scope: BrowserSessionProfileScope, label: string): BrowserSessionProfile | null {
    // Why: block scope:'default' here — only the constructor makes the default profile; a second one sharing the partition breaks delete.
    if (scope === 'default') {
      return null
    }
    const id = randomUUID()
    // Why: deterministic partition-from-id lets main rebuild the allowlist on restart without a separate partition→profile map.
    const partition = getOrcaProfileBrowserSessionPartition(this.activeOrcaProfileId, id)
    const profile: BrowserSessionProfile = {
      id,
      scope,
      partition,
      label,
      source: null
    }
    this.profiles.set(id, profile)
    this.setupSessionPolicies(partition)
    this.persistProfiles()
    return profile
  }

  updateProfileSource(
    profileId: string,
    source: BrowserSessionProfile['source']
  ): BrowserSessionProfile | null {
    const profile = this.profiles.get(profileId)
    if (!profile) {
      return null
    }
    const updated = { ...profile, source }
    this.profiles.set(profileId, updated)
    if (profileId === 'default') {
      this.persistSource(source)
    } else {
      this.persistProfiles()
    }
    return updated
  }

  async deleteProfile(profileId: string): Promise<boolean> {
    const profile = this.profiles.get(profileId)
    if (!profile || profile.scope === 'default') {
      return false
    }
    this.profiles.delete(profileId)
    this.persistProfiles()
    this.grantedMediaOriginsByPartition.delete(profile.partition)
    const meta = this.loadPersistedMeta()
    const pendingCookieImports = { ...meta.pendingCookieImports }
    // Why: deleting the profile must reclaim its staged plaintext cookie DB, not
    // just drop the metadata pointer (which would orphan it forever).
    this.unlinkStagedCookieDb(pendingCookieImports[profile.partition])
    delete pendingCookieImports[profile.partition]
    const userAgentByPartition = { ...meta.userAgentByPartition }
    delete userAgentByPartition[profile.partition]
    this.persistMeta({
      pendingCookieImports,
      pendingCookieDbPath: pendingCookieImports[this.defaultPartition] ?? null,
      userAgentByPartition,
      userAgent: userAgentByPartition[this.defaultPartition] ?? null
    })

    // Why: clear the partition's storage so deleting a profile doesn't leave orphaned cookies/cache behind.
    try {
      const sess = session.fromPartition(profile.partition)
      this.clearSessionPolicies(profile.partition, sess)
      await sess.clearStorageData()
      await sess.clearCache()
    } catch {
      // Why: cleanup is best-effort — the profile is already out of the registry, so will-attach-webview blocks it regardless.
    }
    return true
  }

  // Why: lets users undo a cookie import without deleting the default profile itself.
  async clearDefaultSessionCookies(): Promise<boolean> {
    try {
      // Why: persist metadata before clearing storage so a mid-clear quit doesn't leave a stale "imported from X" badge.
      const defaultProfile = this.profiles.get('default')
      if (defaultProfile) {
        this.profiles.set('default', { ...defaultProfile, source: null })
      }
      const meta = this.loadPersistedMeta()
      const pendingCookieImports = { ...meta.pendingCookieImports }
      // Why: "undo import" must delete the staged plaintext cookie DB, not just
      // the metadata entry — otherwise the exact action to remove the cookies
      // leaves a permanent decrypted copy on disk.
      this.unlinkStagedCookieDb(pendingCookieImports[this.defaultPartition])
      this.grantedMediaOriginsByPartition.delete(this.defaultPartition)
      delete pendingCookieImports[this.defaultPartition]
      const userAgentByPartition = { ...meta.userAgentByPartition }
      delete userAgentByPartition[this.defaultPartition]
      this.persistMeta({
        defaultSource: null,
        userAgent: null,
        userAgentByPartition,
        pendingCookieDbPath: null,
        pendingCookieImports
      })

      const sess = session.fromPartition(this.defaultPartition)
      await sess.clearStorageData({ storages: ['cookies'] })
      return true
    } catch {
      return false
    }
  }

  // Why: validate on-disk profile shape so a tampered JSON file can't inject an arbitrary partition into the will-attach-webview allowlist.
  private isValidPersistedProfile(profile: unknown): profile is BrowserSessionProfile {
    if (!profile || typeof profile !== 'object') {
      return false
    }
    const candidate = profile as Partial<BrowserSessionProfile>
    return (
      candidate.id !== 'default' &&
      candidate.scope !== 'default' &&
      typeof candidate.id === 'string' &&
      typeof candidate.partition === 'string' &&
      typeof candidate.label === 'string' &&
      this.isProfileOwnedSessionPartition(candidate.partition)
    )
  }

  private isProfileOwnedSessionPartition(partition: string): boolean {
    if (
      this.activeOrcaProfileId === DEFAULT_LOCAL_ORCA_PROFILE_ID &&
      LEGACY_BROWSER_SESSION_PARTITION_RE.test(partition)
    ) {
      return true
    }

    const segment = getOrcaProfileBrowserPartitionSegment(this.activeOrcaProfileId)
    const prefix = `persist:orca-profile-${segment}-browser-session-`
    if (!partition.startsWith(prefix)) {
      return false
    }
    const profileId = partition.slice(prefix.length)
    return /^[\da-f-]{8}-[\da-f-]{4}-[\da-f-]{4}-[\da-f-]{4}-[\da-f-]{12}$/.test(profileId)
  }

  hydrateFromPersisted(profiles: BrowserSessionProfile[]): void {
    for (const profile of profiles) {
      if (!this.isValidPersistedProfile(profile)) {
        continue
      }
      this.profiles.set(profile.id, profile)
      if (profile.partition !== this.defaultPartition) {
        this.setupSessionPolicies(profile.partition)
      }
    }
  }

  // Why: one shared installer keeps every partition's deny-by-default permission/download policies from drifting apart.
  private readonly configuredPartitions = new Set<string>()
  private readonly handleWillDownload = (
    _event: Electron.Event,
    item: Electron.DownloadItem,
    webContents: Electron.WebContents
  ): void => {
    browserManager.handleGuestWillDownload({ guestWebContentsId: webContents.id, item })
  }

  private setupSessionPolicies(partition: string): void {
    if (this.configuredPartitions.has(partition)) {
      return
    }

    const sess = session.fromPartition(partition)
    browserManager.installCertificateRequestGuard(sess)
    if (typeof sess.getUserAgent === 'function') {
      const cleanUA = cleanElectronUserAgent(sess.getUserAgent())
      sess.setUserAgent(cleanUA)
      setupClientHintsOverride(sess, cleanUA)
    }
    sess.setPermissionRequestHandler((webContents, permission, callback, details) => {
      // Why: defer media to macOS TCC; denying at the session layer throws NotAllowedError even after the user granted Camera/Mic to the OS.
      if (permission === 'media') {
        this.handleMediaPermissionRequest(
          partition,
          webContents,
          callback,
          details as Electron.MediaAccessPermissionRequest | undefined
        )
        return
      }
      const allowed = isBrowserSessionPermissionAllowed(permission, webContents?.id)
      if (!allowed) {
        browserManager.notifyPermissionDenied({
          guestWebContentsId: webContents.id,
          permission,
          rawUrl: webContents.getURL()
        })
      }
      callback(allowed)
    })
    sess.setPermissionCheckHandler((checkWebContents, permission, requestingOrigin, details) => {
      if (permission === 'media') {
        // Why: an OS-level media grant is not per-origin consent; only report a
        // media permission as held for an origin that actually went through the
        // request handler and was granted.
        const origin = this.mediaOriginKey(requestingOrigin)
        return (
          !!origin &&
          (this.grantedMediaOriginsByPartition.get(partition)?.has(origin) ?? false) &&
          hasSystemMediaAccess(details?.mediaType)
        )
      }
      if (allowsBrowserWebAuthnPermission(permission, details)) {
        return true
      }
      return isBrowserSessionPermissionAllowed(permission, checkWebContents?.id)
    })
    installBrowserWebAuthnAccessHandlers(sess)
    sess.setDisplayMediaRequestHandler((_request, callback) => {
      callback({ video: undefined, audio: undefined })
    })
    sess.removeListener('will-download', this.handleWillDownload)
    sess.on('will-download', this.handleWillDownload)
    this.configuredPartitions.add(partition)
  }

  // Why: keep id/origin captured synchronously — the guest can be destroyed
  // while the OS TCC prompt is showing; reading webContents.id/getURL() after the
  // await would throw and leave the Electron permission callback unresolved (a
  // permanently pending request). Fail closed on any error path instead.
  private handleMediaPermissionRequest(
    partition: string,
    webContents: Electron.WebContents,
    callback: (granted: boolean) => void,
    details: Electron.MediaAccessPermissionRequest | undefined
  ): void {
    let guestWebContentsId: number
    let rawUrl: string
    try {
      guestWebContentsId = webContents.id
      rawUrl = webContents.getURL()
    } catch {
      callback(false)
      return
    }
    const origin = this.mediaOriginKey((details?.securityOrigin as string | undefined) ?? rawUrl)
    if (!origin) {
      this.notifyMediaDenied(guestWebContentsId, rawUrl)
      callback(false)
      return
    }
    void requestSystemMediaAccess(details).then(
      (granted) => {
        if (granted) {
          this.recordMediaGrant(partition, origin)
        } else {
          this.notifyMediaDenied(guestWebContentsId, rawUrl)
        }
        callback(granted)
      },
      (error: unknown) => {
        console.error('[permissions] Browser media access failed:', error)
        this.notifyMediaDenied(guestWebContentsId, rawUrl)
        callback(false)
      }
    )
  }

  private notifyMediaDenied(guestWebContentsId: number, rawUrl: string): void {
    try {
      browserManager.notifyPermissionDenied({
        guestWebContentsId,
        permission: 'media',
        rawUrl
      })
    } catch {
      /* best-effort — the guest may already be gone */
    }
  }

  private recordMediaGrant(partition: string, origin: string): void {
    const origins = this.grantedMediaOriginsByPartition.get(partition) ?? new Set<string>()
    origins.add(origin)
    this.grantedMediaOriginsByPartition.set(partition, origins)
  }

  private mediaOriginKey(rawUrl: string | undefined | null): string | null {
    if (!rawUrl) {
      return null
    }
    try {
      const origin = new URL(rawUrl).origin
      return origin && origin !== 'null' ? origin : null
    } catch {
      return null
    }
  }

  private clearSessionPolicies(partition: string, sess: Session): void {
    // Why: the Electron Session survives partition deletion; clear callbacks/listeners so removed profiles don't retain closures.
    this.configuredPartitions.delete(partition)
    this.grantedMediaOriginsByPartition.delete(partition)
    browserManager.removeCertificateRequestGuard(sess)
    sess.removeListener('will-download', this.handleWillDownload)
    clearBrowserWebAuthnAccessHandlers(sess)
    sess.setPermissionRequestHandler(null)
    sess.setPermissionCheckHandler(null)
    sess.setDisplayMediaRequestHandler(null)
  }
}

export const browserSessionRegistry = new BrowserSessionRegistry()
