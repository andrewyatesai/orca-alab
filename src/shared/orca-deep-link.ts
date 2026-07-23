// orca:// deep-link grammar (#4384): ONE parser shared by main (argv/open-url
// routing), preload/renderer (terminal OSC-8 clicks), and tests. PR1 dispatches
// `focus` only; `worktree`/`pair`/`run` are parsed so the grammar is
// forward-fixed from day one and surface an "unsupported yet" notice until PR2.

/** The scheme the engine is asked to mint OSC-8 hyperlinks for (aterm host capability). */
export const ORCA_DEEP_LINK_SCHEME = 'orca'

export type OrcaDeepLink =
  | { kind: 'focus'; handle: string }
  | { kind: 'worktree'; worktreeId: string; tabId?: string }
  | { kind: 'pair'; code: string }
  | { kind: 'run'; worktreeId: string; command: string; title?: string }

/** Origin is stamped by the TRANSPORT (open-url/argv vs in-pane OSC-8 click),
 *  never parsed from the URL — consent/toast labels depend on it. */
export type OrcaDeepLinkOrigin = { source: 'os' } | { source: 'terminal'; worktreeId: string }

export const MAX_ORCA_DEEP_LINK_LENGTH = 2048
// Why: kills traversal shapes, oversized handles, and injection into the RPC layer before it happens.
export const TERMINAL_HANDLE_PATTERN = /^term_[A-Za-z0-9-]{1,128}$/

export type OrcaDeepLinkUiNotice = 'unrecognized' | 'unsupported' | 'terminal-gone'

/** Payload of the main→renderer `ui:deepLink` channel. `link` events carry the
 *  parsed link for renderer-side handling (consent surface in PR2); `notice`
 *  events request a toast. No renderer→main channel accepts an origin claim. */
export type OrcaDeepLinkUiEvent =
  | { type: 'link'; link: OrcaDeepLink; origin: OrcaDeepLinkOrigin }
  | { type: 'notice'; notice: OrcaDeepLinkUiNotice }

export function parseOrcaDeepLink(raw: string): OrcaDeepLink | null {
  if (typeof raw !== 'string' || raw.length === 0 || raw.length > MAX_ORCA_DEEP_LINK_LENGTH) {
    return null
  }
  let url: URL
  try {
    url = new URL(raw)
  } catch {
    return null
  }
  if (url.protocol !== 'orca:') {
    return null
  }
  // Why: only `pair` may carry secrets and none of the grammar carries identity;
  // credentialed URLs are always hostile shapes.
  if (url.username !== '' || url.password !== '') {
    return null
  }
  // Why: non-special schemes keep the host's original case (WHATWG opaque host);
  // the grammar is case-insensitive per design.
  switch (url.hostname.toLowerCase()) {
    case 'focus':
      return parseFocus(url, raw)
    case 'worktree':
      return parseWorktree(url, raw)
    case 'pair':
      return parsePair(url)
    case 'run':
      return parseRun(url)
    default:
      // Unknown hosts → null (strict-hostname precedent: src/shared/pairing.ts).
      return null
  }
}

// Path of the URL as WRITTEN (before WHATWG dot-segment normalization).
function rawPathOf(raw: string): string {
  const match = /^[a-zA-Z][a-zA-Z0-9+.-]*:\/\/[^/?#]*([^?#]*)/.exec(raw)
  return match?.[1] ?? ''
}

function singlePathSegment(url: URL, raw: string): string | null {
  // Why: URL parsing resolves `..`/`.` before we validate, so a link that
  // DISPLAYS one target could resolve to another — reject any normalized shape.
  if (rawPathOf(raw) !== url.pathname) {
    return null
  }
  if (!url.pathname.startsWith('/')) {
    return null
  }
  const segment = url.pathname.slice(1)
  if (segment === '' || segment.includes('/')) {
    return null
  }
  return segment
}

function parseFocus(url: URL, raw: string): OrcaDeepLink | null {
  const segment = singlePathSegment(url, raw)
  if (!segment || !TERMINAL_HANDLE_PATTERN.test(segment)) {
    return null
  }
  return { kind: 'focus', handle: segment }
}

function parseWorktree(url: URL, raw: string): OrcaDeepLink | null {
  const segment = singlePathSegment(url, raw)
  if (!segment) {
    return null
  }
  let worktreeId: string
  try {
    // Why: worktree ids are `repoId::worktreePath` and MUST be percent-encoded;
    // URL/`decodeURIComponent` only — no hand-rolled decoding.
    worktreeId = decodeURIComponent(segment)
  } catch {
    return null
  }
  if (worktreeId === '') {
    return null
  }
  const tabId = url.searchParams.get('tab')
  return tabId ? { kind: 'worktree', worktreeId, tabId } : { kind: 'worktree', worktreeId }
}

function parsePair(url: URL): OrcaDeepLink | null {
  if (url.pathname !== '' && url.pathname !== '/') {
    return null
  }
  // Why: mirrors src/shared/pairing.ts — query param preferred, fragment fallback
  // (Android intents / Expo Router preserve query params more reliably).
  const code = url.searchParams.get('code') ?? (url.hash ? url.hash.slice(1) : null)
  if (!code) {
    return null
  }
  return { kind: 'pair', code }
}

function parseRun(url: URL): OrcaDeepLink | null {
  if (url.pathname !== '' && url.pathname !== '/') {
    return null
  }
  const worktreeId = url.searchParams.get('worktree')
  // `cmd` is decoded here but NEVER interpreted by the parser.
  const command = url.searchParams.get('cmd')
  if (!worktreeId || !command) {
    return null
  }
  const title = url.searchParams.get('title')
  return title ? { kind: 'run', worktreeId, command, title } : { kind: 'run', worktreeId, command }
}

/** Loggable form: `pair` URLs carry auth material and are always redacted. */
export function describeOrcaDeepLinkForLog(link: OrcaDeepLink): string {
  switch (link.kind) {
    case 'focus':
      return `focus/${link.handle}`
    case 'worktree':
      return `worktree/${link.worktreeId}${link.tabId ? `?tab=${link.tabId}` : ''}`
    case 'pair':
      return 'pair?code=<redacted>'
    case 'run':
      return `run?worktree=${link.worktreeId}`
  }
}
