// Why: every release gate must agree on the tags GitHub and the updater treat as desktop cuts.
const VERSION_COMPONENT = '(?:0|[1-9][0-9]*)'
const DESKTOP_RELEASE_TAG_PATTERN = new RegExp(
  `^v([1-9][0-9]*)\\.(${VERSION_COMPONENT})\\.(${VERSION_COMPONENT})(?:-rc\\.(${VERSION_COMPONENT})|-fork\\.([1-9][0-9]*))?$`
)

function safeVersionNumber(value) {
  const parsed = Number(value)
  return Number.isSafeInteger(parsed) ? parsed : null
}

export function parseDesktopReleaseTag(tag) {
  if (typeof tag !== 'string') {
    return null
  }
  const match = DESKTOP_RELEASE_TAG_PATTERN.exec(tag)
  if (!match) {
    return null
  }
  const [major, minor, patch, rc, fork] = match
    .slice(1)
    .map((value) => (value === undefined ? null : safeVersionNumber(value)))
  if (
    [major, minor, patch].includes(null) ||
    (match[4] && rc === null) ||
    (match[5] && fork === null)
  ) {
    return null
  }
  return { tag, major, minor, patch, rc, fork }
}

export function desktopReleaseVersion(tag) {
  return parseDesktopReleaseTag(tag) ? tag.slice(1) : null
}

export function isDesktopReleasePrerelease(tag) {
  const parsed = parseDesktopReleaseTag(tag)
  return parsed ? parsed.rc !== null : false
}
