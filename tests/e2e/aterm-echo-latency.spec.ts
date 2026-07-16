import { randomUUID } from 'node:crypto'
import { rmSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import type { Page } from '@stablyai/playwright-test'
import { test, expect } from './helpers/orca-app'
import { sendToTerminal } from './helpers/terminal'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import {
  ensureActiveWorktreePaneLoad,
  focusActiveTerminalInput,
  focusPane,
  getTerminalContentForPtyId
} from './artificial-opencode-pane-interactions'
import { nodeTerminalCommand } from './terminal-node-command'
import { buildFreshShellProbeInputSequence } from './terminal-probe-input-sequence'
import { waitForPtyShellEcho } from './terminal-pty-readiness'
import { stripSerializedControlSequences } from './terminal-serialized-text'

// HONEST end-to-end keystroke echo-latency measurement, run inside the REAL
// Electron app: trusted CDP keydown → terminal input encode → pty.write IPC →
// main/daemon transport → real PTY → raw-mode echo process → PTY read →
// transport back → renderer parse → the first requestAnimationFrame tick whose
// parsed buffer contains the echoed char. Reported Dan Luu-style
// (min/p50/p95/p99/max), idle AND under a sustained background output flood in
// a second pane. It is a MEASUREMENT (loose sanity asserts only), not a perf gate.
//
// Calibration per rust/aterm/docs/FASTER_THAN_GHOSTTY_PLAN.md (ARENA-LAT):
// this is tier (c) — app-internal software keypress→pixel-schedule. It
// INCLUDES the pty round-trip, transport, and parse that
// tests/e2e/aterm-latency.spec.ts (render-half only) explicitly excludes; it
// EXCLUDES the physical keyboard/OS input path, GPU execute/present,
// compositor, and display scanout — so these numbers are NOT comparable to
// typometer (tier a) or camera (tier b) results.
//
// Clock consistency: t_input (keydown event.timeStamp) and t_visible (rAF
// callback timestamp) are both renderer performance-clock values, so no
// cross-process clock mapping is involved. t_visible quantizes to the frame
// the echoed glyph is first scheduled to paint, which is the point.
//
// The echoing foreground process is a raw-mode node script that writes each
// received char back verbatim (same round-trip shape as shell echo, without
// zsh/PS1 redraw noise) and wipes the screen on Enter so every measured char
// is detected against a viewport that cannot already contain it — the
// established typing-latency precedent (terminal-typing-latency.spec.ts).

const SAMPLES_PER_CONDITION = 120
const BATCH_SIZE = 24
const PER_KEY_TIMEOUT_MS = 10_000
// Lowercase+digits only: layout-independent single keydowns whose event.key
// equals the echoed glyph on every platform. Uniqueness is only required
// within one batch (the screen is wiped between batches), and any 24
// consecutive chars of this 36-char cycle are distinct.
const ECHO_ALPHABET = 'abcdefghijklmnopqrstuvwxyz0123456789'
// Loose pathology bounds, not perf gates (an echo median near these values
// means something is broken, not slow).
const IDLE_MEDIAN_SANITY_MS = 150
const LOADED_MEDIAN_SANITY_MS = 1_500
// Paced flood matching the sustained-load house precedent
// (sustained-agent-typing-load-scripts.ts): drain-aware writes on a fixed
// tick, so the load is a steady stream instead of one giant buffered blob.
const FLOOD_RATE_KBPS = 256
const FLOOD_MAX_DURATION_S = 300

type EchoLatencyStats = {
  samples: number
  minMs: number
  medianMs: number
  p95Ms: number
  p99Ms: number
  maxMs: number
  meanMs: number
}
type EchoSample = { tInputMs: number; tVisibleMs: number }
type EchoLatencyHarness = {
  viewportText(ptyId: string): string
  arm(char: string, ptyId: string): void
  wait(timeoutMs: number): Promise<EchoSample>
}
type EchoLatencyWindow = Window & {
  __orcaEchoLatency?: EchoLatencyHarness
  __atermWorkerRender?: boolean
}
type PaneBufferProbe = {
  terminal?: {
    rows?: number
    buffer?: {
      active?: {
        baseY: number
        getLine(absY: number): { translateToString(trim?: boolean): string } | undefined
      }
    }
  }
}

function echoProcessScript(markerId: string): string {
  return `
process.stdin.setEncoding('utf8')
if (process.stdin.isTTY) process.stdin.setRawMode(true)
process.stdin.resume()
const interrupt = String.fromCharCode(3)
process.stdout.write('ECHO_READY_${markerId}\\r\\n')
process.stdin.on('data', (chunk) => {
  if (chunk.includes(interrupt)) {
    process.exit(0)
  }
  let out = ''
  for (const char of chunk) {
    out += char === '\\r' || char === '\\n' ? '\\x1b[2J\\x1b[H' : char
  }
  process.stdout.write(out)
})
`
}

function floodProcessScript(markerId: string): string {
  return `
process.stdin.setEncoding('utf8')
if (process.stdin.isTTY) process.stdin.setRawMode(true)
process.stdin.resume()
const interrupt = String.fromCharCode(3)
const TICK_MS = 20
const bytesPerTick = Math.max(1, Math.floor((${FLOOD_RATE_KBPS} * 1024 * TICK_MS) / 1000))
const writeChunk = (data) =>
  new Promise((resolve) => {
    if (process.stdout.write(data)) {
      resolve()
    } else {
      process.stdout.once('drain', resolve)
    }
  })
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
async function flood() {
  const deadline = Date.now() + ${FLOOD_MAX_DURATION_S} * 1000
  let seq = 0
  while (Date.now() < deadline) {
    let chunk = ''
    while (chunk.length < bytesPerTick) {
      seq += 1
      chunk += 'FLOOD_${markerId} ' + seq + ' ' + 'x'.repeat(64) + '\\r\\n'
    }
    await writeChunk(chunk)
    await sleep(TICK_MS)
  }
  process.exit(0)
}
let started = false
process.stdout.write('FLOOD_READY_${markerId}\\r\\n')
process.stdin.on('data', (chunk) => {
  if (chunk.includes(interrupt)) {
    process.exit(0)
  }
  if (!started) {
    started = true
    void flood()
  }
})
`
}

// The whole harness lives in the page so both timestamps come from one clock
// and no Node↔CDP round-trip sits between keydown and echo detection.
async function installEchoLatencyHarness(page: Page): Promise<void> {
  await page.evaluate(() => {
    const w = window as unknown as EchoLatencyWindow
    if (w.__orcaEchoLatency) {
      return
    }

    const viewportText = (ptyId: string): string => {
      for (const manager of window.__paneManagers?.values() ?? []) {
        for (const pane of manager.getPanes?.() ?? []) {
          if (pane.container?.dataset?.ptyId !== ptyId) {
            continue
          }
          // Bottom-screen rows [baseY, baseY+rows): the echo always lands at
          // the cursor, and the facade serves display rows regardless of
          // scrollback growth. Same read path as getTerminalLogicalText.
          const terminal = (pane as unknown as PaneBufferProbe).terminal
          const buffer = terminal?.buffer?.active
          if (!buffer) {
            return ''
          }
          const rows = terminal?.rows ?? 0
          const lines: string[] = []
          for (let absY = buffer.baseY; absY < buffer.baseY + rows; absY++) {
            lines.push(buffer.getLine(absY)?.translateToString(true) ?? '')
          }
          return lines.join('\n')
        }
      }
      return ''
    }

    type Pending = {
      char: string
      ptyId: string
      tInputMs: number | null
      result: EchoSample | null
      error: string | null
    }
    let pending: Pending | null = null

    // Capture phase so the trusted keydown is timestamped before the
    // terminal's own textarea handler consumes it.
    window.addEventListener(
      'keydown',
      (event) => {
        if (pending && pending.tInputMs === null && event.key === pending.char) {
          pending.tInputMs = event.timeStamp
        }
      },
      true
    )

    let rafId: number | null = null
    const onFrame = (frameTs: number): void => {
      rafId = null
      const current = pending
      if (!current || current.result || current.error) {
        return
      }
      if (current.tInputMs !== null && viewportText(current.ptyId).includes(current.char)) {
        current.result = { tInputMs: current.tInputMs, tVisibleMs: frameTs }
        return
      }
      rafId = requestAnimationFrame(onFrame)
    }

    w.__orcaEchoLatency = {
      viewportText,
      arm(char, ptyId) {
        // A pre-visible char would resolve at 0ms; the batch protocol wipes
        // the screen so this only fires on genuine protocol breakage.
        const alreadyVisible = viewportText(ptyId).includes(char)
        pending = {
          char,
          ptyId,
          tInputMs: null,
          result: null,
          error: alreadyVisible
            ? `char ${JSON.stringify(char)} already visible before its keystroke`
            : null
        }
        if (rafId === null) {
          rafId = requestAnimationFrame(onFrame)
        }
      },
      async wait(timeoutMs) {
        const start = performance.now()
        for (;;) {
          const current = pending
          if (!current) {
            throw new Error('echo-latency wait() called with nothing armed')
          }
          if (current.error) {
            pending = null
            throw new Error(current.error)
          }
          if (current.result) {
            pending = null
            return current.result
          }
          if (performance.now() - start > timeoutMs) {
            const tail = viewportText(current.ptyId).slice(-200)
            const inputState = current.tInputMs === null ? 'never fired' : 'captured'
            pending = null
            throw new Error(
              `echo of ${JSON.stringify(current.char)} not visible within ${timeoutMs}ms (tInput=${inputState}; viewport tail: ${JSON.stringify(tail)})`
            )
          }
          await new Promise((resolve) => setTimeout(resolve, 10))
        }
      }
    }
  })
}

async function launchTerminalScript(page: Page, ptyId: string, scriptPath: string): Promise<void> {
  for (const input of buildFreshShellProbeInputSequence(`${nodeTerminalCommand([scriptPath])}\r`)) {
    await sendToTerminal(page, ptyId, input)
  }
}

// A narrow split pane soft-wraps markers and serialize splits them with
// cursor-move controls, so strip those before matching (waitForPtyShellEcho's
// documented convention).
async function waitForPtyMarker(
  page: Page,
  ptyId: string,
  marker: string,
  timeoutMs: number
): Promise<void> {
  await expect
    .poll(
      async () =>
        stripSerializedControlSequences(await getTerminalContentForPtyId(page, ptyId)).includes(
          marker
        ),
      { timeout: timeoutMs, message: `Terminal PTY ${ptyId} did not contain "${marker}"` }
    )
    .toBe(true)
}

// Enter makes the echo process wipe its screen, guaranteeing the next batch's
// chars are detected against a viewport that cannot already contain them.
async function clearEchoViewport(page: Page, ptyId: string): Promise<void> {
  await sendToTerminal(page, ptyId, '\r')
  await expect
    .poll(
      () =>
        page.evaluate((ptyId) => {
          const harness = (window as unknown as EchoLatencyWindow).__orcaEchoLatency
          return harness ? harness.viewportText(ptyId).trim() : 'harness-missing'
        }, ptyId),
      { timeout: 10_000, message: 'echo pane viewport did not clear between batches' }
    )
    .toBe('')
}

async function measureEchoLatencyPhase(
  page: Page,
  ptyId: string,
  charOffset: number
): Promise<number[]> {
  const latencies: number[] = []
  for (let index = 0; index < SAMPLES_PER_CONDITION; index++) {
    if (index % BATCH_SIZE === 0) {
      await clearEchoViewport(page, ptyId)
    }
    const char = ECHO_ALPHABET[(charOffset + index) % ECHO_ALPHABET.length] ?? 'a'
    await page.evaluate(
      ({ char, ptyId }) => {
        const harness = (window as unknown as EchoLatencyWindow).__orcaEchoLatency
        if (!harness) {
          throw new Error('echo-latency harness is not installed')
        }
        harness.arm(char, ptyId)
      },
      { char, ptyId }
    )
    await page.keyboard.type(char)
    const sample = await page.evaluate((timeoutMs) => {
      const harness = (window as unknown as EchoLatencyWindow).__orcaEchoLatency
      if (!harness) {
        throw new Error('echo-latency harness is not installed')
      }
      return harness.wait(timeoutMs)
    }, PER_KEY_TIMEOUT_MS)
    latencies.push(sample.tVisibleMs - sample.tInputMs)
  }
  return latencies
}

function summarizeLatencies(latencies: number[]): EchoLatencyStats {
  const sorted = [...latencies].sort((a, b) => a - b)
  const quantile = (q: number): number =>
    sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil(q * sorted.length) - 1))] ?? 0
  const total = sorted.reduce((sum, value) => sum + value, 0)
  return {
    samples: sorted.length,
    minMs: sorted[0] ?? 0,
    medianMs: quantile(0.5),
    p95Ms: quantile(0.95),
    p99Ms: quantile(0.99),
    maxMs: sorted.at(-1) ?? 0,
    meanMs: sorted.length > 0 ? total / sorted.length : 0
  }
}

test.describe('aterm end-to-end echo latency @aterm-echo-latency', () => {
  test('measures keydown→echo-visible percentiles idle and under load', async ({
    orcaPage,
    testRepoPath
  }, testInfo) => {
    // ~240 awaited keystroke round-trips plus app startup and two script
    // launches; the loaded phase is deliberately slow.
    test.setTimeout(420_000)

    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    // Both panes exist for BOTH conditions so the measured pane's geometry
    // (wrap width, render cost) is identical idle vs loaded.
    const [measuredPane, floodPane] = await ensureActiveWorktreePaneLoad(orcaPage, 2)
    if (!measuredPane?.ptyId || !floodPane?.ptyId) {
      throw new Error('expected two PTY-bound terminal panes for the echo-latency bench')
    }
    await waitForPtyShellEcho(orcaPage, measuredPane.ptyId, 30_000)
    await waitForPtyShellEcho(orcaPage, floodPane.ptyId, 30_000)

    const runId = randomUUID()
    // Short marker id: full-UUID markers can soft-wrap in a narrow split pane
    // and defeat even the stripped-serialize match.
    const markerId = runId.slice(0, 8)
    const echoScriptPath = path.join(testRepoPath, `.orca-echo-latency-echo-${runId}.mjs`)
    const floodScriptPath = path.join(testRepoPath, `.orca-echo-latency-flood-${runId}.mjs`)
    writeFileSync(echoScriptPath, echoProcessScript(markerId))
    writeFileSync(floodScriptPath, floodProcessScript(markerId))

    let scriptsLaunched = false
    try {
      await launchTerminalScript(orcaPage, measuredPane.ptyId, echoScriptPath)
      scriptsLaunched = true
      await waitForPtyMarker(orcaPage, measuredPane.ptyId, `ECHO_READY_${markerId}`, 15_000)
      // The flood process launches now but idles at READY until poked, so the
      // idle phase truly has no background output.
      await launchTerminalScript(orcaPage, floodPane.ptyId, floodScriptPath)
      await waitForPtyMarker(orcaPage, floodPane.ptyId, `FLOOD_READY_${markerId}`, 15_000)

      await installEchoLatencyHarness(orcaPage)
      await focusPane(orcaPage, measuredPane.paneKey)
      await focusActiveTerminalInput(orcaPage)

      const idleLatencies = await measureEchoLatencyPhase(orcaPage, measuredPane.ptyId, 0)

      await sendToTerminal(orcaPage, floodPane.ptyId, 'g')
      await waitForPtyMarker(orcaPage, floodPane.ptyId, `FLOOD_${markerId}`, 15_000)
      // Two spaced reads that differ prove the flood is streaming, so the
      // "loaded" label is earned rather than assumed.
      const floodContentBefore = await getTerminalContentForPtyId(orcaPage, floodPane.ptyId)
      await orcaPage.waitForTimeout(250)
      const floodContentAfter = await getTerminalContentForPtyId(orcaPage, floodPane.ptyId)
      expect(floodContentAfter !== floodContentBefore, 'background flood is streaming').toBe(true)

      await focusPane(orcaPage, measuredPane.paneKey)
      await focusActiveTerminalInput(orcaPage)
      const loadedLatencies = await measureEchoLatencyPhase(
        orcaPage,
        measuredPane.ptyId,
        SAMPLES_PER_CONDITION
      )

      await sendToTerminal(orcaPage, floodPane.ptyId, '\x03')

      const environment = await orcaPage.evaluate((ptyId) => {
        const w = window as unknown as EchoLatencyWindow
        for (const manager of window.__paneManagers?.values() ?? []) {
          for (const pane of manager.getPanes?.() ?? []) {
            if (pane.container?.dataset?.ptyId === ptyId) {
              const terminal = pane.terminal as unknown as { cols?: number; rows?: number }
              return {
                workerRender: w.__atermWorkerRender !== false,
                cols: terminal?.cols ?? 0,
                rows: terminal?.rows ?? 0
              }
            }
          }
        }
        return { workerRender: w.__atermWorkerRender !== false, cols: 0, rows: 0 }
      }, measuredPane.ptyId)

      const idle = summarizeLatencies(idleLatencies)
      const loaded = summarizeLatencies(loadedLatencies)
      const summary = {
        tier: 'c-app-internal (FASTER_THAN_GHOSTTY_PLAN.md ARENA-LAT)',
        includes: 'keydown timestamp → pty round-trip → transport → parse → first rAF tick',
        excludes: 'physical keyboard / OS input path, GPU present, compositor, display scanout',
        platform: process.platform,
        environment,
        floodRateKBps: FLOOD_RATE_KBPS,
        idle,
        loaded
      }
      const result = {
        ...summary,
        idleLatenciesMs: idleLatencies.map((value) => Number(value.toFixed(3))),
        loadedLatenciesMs: loadedLatencies.map((value) => Number(value.toFixed(3)))
      }

      const fmtStats = (s: EchoLatencyStats): string =>
        `min ${s.minMs.toFixed(1)} | p50 ${s.medianMs.toFixed(1)} | p95 ${s.p95Ms.toFixed(1)} | p99 ${s.p99Ms.toFixed(1)} | max ${s.maxMs.toFixed(1)}  (mean ${s.meanMs.toFixed(1)}, n=${s.samples})`
      const lines = [
        `[aterm-echo-latency] tier (c) app-internal keydown→echo-visible, ms — NOT camera/typometer-comparable`,
        `[aterm-echo-latency] pane ${environment.cols}x${environment.rows}, workerRender=${environment.workerRender}, flood ${FLOOD_RATE_KBPS}KB/s, ${process.platform}`,
        `[aterm-echo-latency] idle   : ${fmtStats(idle)}`,
        `[aterm-echo-latency] loaded : ${fmtStats(loaded)}`
      ]
      // eslint-disable-next-line no-console
      console.log(`\n${lines.join('\n')}\n`)
      // eslint-disable-next-line no-console
      console.log(`[aterm-echo-latency] RESULT_JSON ${JSON.stringify(summary)}`)
      testInfo.annotations.push({ type: 'aterm-echo-latency', description: lines.join(' | ') })
      await testInfo.attach('aterm-echo-latency.json', {
        body: JSON.stringify(result, null, 2),
        contentType: 'application/json'
      })

      // Loose sanity asserts only — this is a measurement, not a perf gate.
      expect(idle.samples, 'idle sample count').toBeGreaterThanOrEqual(100)
      expect(loaded.samples, 'loaded sample count').toBeGreaterThanOrEqual(100)
      expect(idle.medianMs, 'idle median is positive').toBeGreaterThan(0)
      expect(loaded.medianMs, 'loaded median is positive').toBeGreaterThan(0)
      expect(Number.isFinite(idle.p99Ms), 'idle p99 finite').toBe(true)
      expect(Number.isFinite(loaded.p99Ms), 'loaded p99 finite').toBe(true)
      expect(
        idle.medianMs,
        `idle echo median under ${IDLE_MEDIAN_SANITY_MS}ms (pathology bound)`
      ).toBeLessThan(IDLE_MEDIAN_SANITY_MS)
      expect(
        loaded.medianMs,
        `loaded echo median under ${LOADED_MEDIAN_SANITY_MS}ms (pathology bound)`
      ).toBeLessThan(LOADED_MEDIAN_SANITY_MS)
    } finally {
      if (scriptsLaunched) {
        await sendToTerminal(orcaPage, floodPane.ptyId, '\x03').catch(() => undefined)
        await sendToTerminal(orcaPage, measuredPane.ptyId, '\x03').catch(() => undefined)
      }
      rmSync(echoScriptPath, { force: true })
      rmSync(floodScriptPath, { force: true })
    }
  })
})
