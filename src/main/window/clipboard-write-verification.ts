// Verified clipboard text writes (PC-5611/8977): Electron's clipboard.writeText
// returns void even when the OS write silently fails (the Windows 10 "says
// copied, clipboard empty" family), so every allowed write path used to swallow
// failures. Write, read back, compare; retry once on mismatch (classic Win32
// open-clipboard contention — the leading suspect for the upstream reports) and
// return whether the final read-back matched, so callers can surface the outcome.

export type ClipboardWriteTarget = 'clipboard' | 'selection'

export type VerifiedClipboardWriteDeps = {
  write: (text: string) => void
  /** Read back from the SAME target the write went to ('selection' verifies via
   *  clipboard.readText('selection'), never the default clipboard). */
  read: () => string
  /** The retry back-off; injectable so tests don't sleep. */
  delay?: (ms: number) => Promise<void>
  /** Structured-warning sink; injectable for tests. */
  warn?: (message: string, details: Record<string, unknown>) => void
}

// Payloads beyond this use the bounded compare: length + head/tail edges. The
// read itself is one clipboard.readText call either way (Electron exposes no
// partial read) — the bound is on the COMPARISON, never a second read.
const BOUNDED_COMPARE_THRESHOLD_CHARS = 256 * 1024
const BOUNDED_COMPARE_EDGE_CHARS = 4 * 1024
const RETRY_DELAY_MS = 150

function defaultDelay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

function clipboardTextMatches(expected: string, actual: string): boolean {
  if (expected.length <= BOUNDED_COMPARE_THRESHOLD_CHARS) {
    return actual === expected
  }
  return (
    actual.length === expected.length &&
    actual.startsWith(expected.slice(0, BOUNDED_COMPARE_EDGE_CHARS)) &&
    actual.endsWith(expected.slice(-BOUNDED_COMPARE_EDGE_CHARS))
  )
}

/**
 * Write `text` and verify it landed by reading the same target back. One retry
 * after ~150ms on mismatch. Returns true when the read-back matches; false means
 * UNVERIFIED — either the write really failed or something (e.g. a clipboard
 * manager) rewrote the contents between write and read-back.
 */
export async function writeClipboardTextVerified(
  text: string,
  target: ClipboardWriteTarget,
  deps: VerifiedClipboardWriteDeps
): Promise<boolean> {
  deps.write(text)
  if (clipboardTextMatches(text, deps.read())) {
    return true
  }
  await (deps.delay ?? defaultDelay)(RETRY_DELAY_MS)
  deps.write(text)
  if (clipboardTextMatches(text, deps.read())) {
    return true
  }
  // Why "unverified", not "failed": the upstream root cause was never identified;
  // this structured warning is the observability the issues asked for.
  const warn =
    deps.warn ??
    ((message: string, details: Record<string, unknown>) => console.warn(message, details))
  warn('[clipboard] text write could not be verified by read-back', {
    target,
    length: text.length,
    platform: process.platform
  })
  return false
}
