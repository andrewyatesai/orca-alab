const AUTO_GRANTED_BROWSER_PERMISSIONS = new Set([
  'fullscreen',
  // Why: 'clipboard-read' is intentionally NOT blanket auto-granted — a blanket grant
  // lets any loaded origin silently read the user's system clipboard (passwords,
  // tokens) via navigator.clipboard.readText(). It is granted only command-scoped
  // while the trusted `orca clipboard read` command is in flight (see below).
  'clipboard-sanitized-write',
  // User-opened browser pages need these profile-scoped grants to complete
  // normal site flows like web push setup and durable app storage.
  'notifications',
  // Chromium can request this at runtime even though Electron's TS union does
  // not list it; chatgpt.com uses it to keep browser storage from eviction.
  'persistent-storage',
  // Chromium still requires user activation, so this only removes Orca's
  // otherwise unactionable denial for immersive browser apps.
  'pointerLock'
])

export function isAutoGrantedBrowserSessionPermission(permission: string): boolean {
  return AUTO_GRANTED_BROWSER_PERMISSIONS.has(permission)
}

// Why: the trusted `orca clipboard read` command drives navigator.clipboard.readText()
// in the inspected page via CDP Runtime.evaluate, which still hits Electron's session
// permission handler. Rather than blanket-grant clipboard-read (letting any page exfil
// the clipboard), AgentBrowserBridge.clipboardRead brackets its evaluate with a
// per-webContents in-flight grant that the permission handler consults; ordinary
// page-initiated reads stay denied. Refcounted so overlapping commands can't revoke early.
const inFlightTrustedClipboardReads = new Map<number, number>()

export function beginTrustedClipboardRead(webContentsId: number): void {
  inFlightTrustedClipboardReads.set(
    webContentsId,
    (inFlightTrustedClipboardReads.get(webContentsId) ?? 0) + 1
  )
}

export function endTrustedClipboardRead(webContentsId: number): void {
  const next = (inFlightTrustedClipboardReads.get(webContentsId) ?? 0) - 1
  if (next > 0) {
    inFlightTrustedClipboardReads.set(webContentsId, next)
  } else {
    inFlightTrustedClipboardReads.delete(webContentsId)
  }
}

export function isTrustedClipboardReadInFlight(webContentsId: number | undefined): boolean {
  return webContentsId !== undefined && (inFlightTrustedClipboardReads.get(webContentsId) ?? 0) > 0
}

// Why: single choke point for the effective grant so the request and check handlers
// agree — clipboard-read is allowed only while a trusted command is in flight for the
// exact webContents that issued it; everything else defers to the static auto-grant set.
export function isBrowserSessionPermissionAllowed(
  permission: string,
  webContentsId: number | undefined
): boolean {
  if (permission === 'clipboard-read') {
    return isTrustedClipboardReadInFlight(webContentsId)
  }
  return isAutoGrantedBrowserSessionPermission(permission)
}
