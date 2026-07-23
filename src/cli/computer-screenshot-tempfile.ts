// Why: computer-use --json exports screenshot bytes to a temp file instead of
// inlining base64; this owns that file's lifecycle (safe temp dir, TTL cleanup,
// filename sanitization), kept apart from the pure output formatting.
import {
  chmodSync,
  lstatSync,
  mkdirSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

export const COMPUTER_SCREENSHOT_TTL_MS = 24 * 60 * 60 * 1000
const COMPUTER_SCREENSHOT_CLEANUP_INTERVAL_MS = 60 * 60 * 1000
const COMPUTER_SCREENSHOT_CLEANUP_MARKER = '.last-cleanup'

export function computerScreenshotTempDir(): string {
  const outputDir =
    process.env.ORCA_COMPUTER_SCREENSHOT_TMPDIR || join(tmpdir(), 'orca-computer-use')
  mkdirSync(outputDir, { recursive: true, mode: 0o700 })
  const stat = lstatSync(outputDir)
  if (!stat.isDirectory() || stat.isSymbolicLink()) {
    throw new Error(`Unsafe computer screenshot temp path: ${outputDir}`)
  }
  if (typeof process.getuid === 'function' && stat.uid !== process.getuid()) {
    throw new Error(`Computer screenshot temp path is not owned by the current user: ${outputDir}`)
  }
  chmodSync(outputDir, 0o700)
  return outputDir
}

export function cleanupComputerScreenshots(outputDir: string): void {
  const now = Date.now()
  const markerPath = join(outputDir, COMPUTER_SCREENSHOT_CLEANUP_MARKER)
  try {
    // Why: agents can call computer-use CLI commands in loops; a marker keeps
    // temp cleanup from becoming a synchronous directory scan per screenshot.
    if (statSync(markerPath).mtimeMs > now - COMPUTER_SCREENSHOT_CLEANUP_INTERVAL_MS) {
      return
    }
  } catch {
    // Missing or unreadable marker means this process should attempt cleanup.
  }

  const cutoff = now - COMPUTER_SCREENSHOT_TTL_MS
  for (const entry of readdirSync(outputDir)) {
    if (!entry.endsWith('-screenshot.png') && !entry.endsWith('-screenshot.img')) {
      continue
    }
    const path = join(outputDir, entry)
    try {
      if (statSync(path).mtimeMs < cutoff) {
        rmSync(path, { force: true })
      }
    } catch {
      // Best-effort cleanup only; formatting should not fail because a temp file raced.
    }
  }
  try {
    writeFileSync(markerPath, `${now}\n`, { mode: 0o600 })
  } catch {
    // Best-effort marker only; stale cleanup state should not hide a screenshot.
  }
}

export function safeCliFileStem(value: string): string {
  return value.replaceAll(/[^a-zA-Z0-9._-]/g, '_')
}
