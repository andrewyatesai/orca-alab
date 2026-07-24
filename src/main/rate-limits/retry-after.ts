// Why: a corrupt/hostile Retry-After must not gate usage refreshes for days (#9617).
const MAX_RETRY_AFTER_MS = 24 * 60 * 60 * 1000

// Why: shared bounded parser so every provider's 429 handling gates the endpoint
// identically — numeric seconds or HTTP-date, rejecting <=0/NaN/Infinity and
// clamping to <=24h — instead of each fetcher re-deriving (and skipping) it.
export function parseRetryAfterMs(header: string | null): number | null {
  if (!header) {
    return null
  }
  const trimmed = header.trim()
  if (!trimmed) {
    return null
  }
  const seconds = Number(trimmed)
  if (Number.isFinite(seconds)) {
    return seconds > 0 ? Math.min(seconds * 1000, MAX_RETRY_AFTER_MS) : null
  }
  // Why: Retry-After may also be an HTTP-date (RFC 9110).
  const dateMs = Date.parse(trimmed)
  if (!Number.isFinite(dateMs)) {
    return null
  }
  const delta = dateMs - Date.now()
  return delta > 0 ? Math.min(delta, MAX_RETRY_AFTER_MS) : null
}
