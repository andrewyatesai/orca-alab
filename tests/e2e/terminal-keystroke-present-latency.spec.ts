import type { Page } from '@stablyai/playwright-test'
import { randomUUID } from 'node:crypto'
import { mkdirSync, rmSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { test, expect } from './helpers/orca-app'
import {
  focusActiveTerminalInput,
  waitForActivePanePtyId,
  waitForActiveTerminalManager,
  waitForTerminalOutput,
  sendToTerminal
} from './helpers/terminal'
import { ensureTerminalVisible, waitForActiveWorktree, waitForSessionReady } from './helpers/store'

// PRECISE keystroke->present latency for the SHIPPED aterm renderer.
//
// The coarse guard (terminal-typing-latency.spec.ts) polls the LOGICAL grid
// every 5ms with a 250ms budget — it measures grid mutation at 5ms granularity,
// not the PRESENT, so it can't tell 7ms from 30ms. This spec instead correlates
// each keystroke to the frame that ACTUALLY blitted its echoed glyph:
//
//   T0 = renderer performance.now() at the keydown (earliest app-visible point
//        of the keystroke), captured by a window-capture listener so it shares
//        the SAME clock as the present log (both in the renderer main world).
//   G  = window.__atermContentGen — the engine content generation, bumped once
//        per PTY process() call — read the instant the unique echo marker lands
//        in the grid.
//   T1 = the first window.__atermPresentLog entry with gen >= G: the real canvas
//        present (CPU putImageData / GPU render()) that first showed that content.
//   latency = T1 - T0.
//
// The probe (aterm-present-latency-probe.ts) is flag-gated on
// window.__ORCA_LATENCY_PROBE, so production and non-opted-in specs pay nothing.

const TOTAL_KEYSTROKES = 55
const WARMUP_KEYSTROKES = 10
const PER_KEYSTROKE_BUDGET_MS = 3_000
const ALPHABET = 'abcdefghijklmnopqrstuvwxyz'

type PresentSample = { t: number; gen: number }
type KeydownSample = { t: number; key: string }

function interactivePromptScript(runId: string): string {
  // Echo each key on its own line with a unique per-key marker so the spec can
  // wait for exactly the echo caused by keystroke N. One process.stdout.write =
  // one PTY chunk = one engine process() (one content-gen bump).
  return `
process.stdin.setEncoding('utf8')
if (process.stdin.isTTY) process.stdin.setRawMode(true)
process.stdin.resume()
let seq = 0
const interrupt = String.fromCharCode(3)
process.stdout.write('\\x1b]0;Keystroke present latency\\x07')
process.stdout.write('LAT_READY_${runId}\\n')
process.stdin.on('data', (chunk) => {
  if (chunk.includes(interrupt)) {
    process.exit(0)
  }
  for (const char of chunk) {
    if (char === '\\r' || char === '\\n') continue
    seq += 1
    process.stdout.write('\\r\\x1b[2Kkey ' + seq + ': ' + char + ' LAT_KEY_${runId}_' + seq + '\\n')
  }
})
`
}

/** Turn the probe on, reset its ring, and install a window-capture keydown clock
 *  so T0 is sampled in the same renderer time origin as the present log. */
async function armLatencyProbe(page: Page): Promise<void> {
  await page.evaluate(() => {
    const w = window as unknown as {
      __ORCA_LATENCY_PROBE?: boolean
      __atermContentGen?: number
      __atermPresentLog?: unknown[]
      __atermKeydownLog?: { t: number; key: string }[]
      __atermKeydownListenerInstalled?: boolean
    }
    w.__ORCA_LATENCY_PROBE = true
    w.__atermContentGen = 0
    w.__atermPresentLog = []
    w.__atermKeydownLog = []
    if (!w.__atermKeydownListenerInstalled) {
      w.__atermKeydownListenerInstalled = true
      // Capture phase on window: fires before any app handler can stopPropagation,
      // so every keystroke is stamped at its earliest renderer-visible moment.
      window.addEventListener(
        'keydown',
        (e) => {
          ;(w.__atermKeydownLog ??= []).push({ t: performance.now(), key: e.key })
        },
        true
      )
    }
  })
}

async function readContentGen(page: Page): Promise<number> {
  return page.evaluate(
    () => (window as unknown as { __atermContentGen?: number }).__atermContentGen ?? 0
  )
}

async function readPresentLog(page: Page): Promise<PresentSample[]> {
  return page.evaluate(
    () => (window as unknown as { __atermPresentLog?: PresentSample[] }).__atermPresentLog ?? []
  )
}

async function readKeydownLog(page: Page): Promise<KeydownSample[]> {
  return page.evaluate(
    () => (window as unknown as { __atermKeydownLog?: KeydownSample[] }).__atermKeydownLog ?? []
  )
}

/** Wait until a real present has drawn content at least as fresh as `gen`. The
 *  process pump schedules a draw and the 33ms backstop guarantees it fires even
 *  headless, so this resolves within a frame or two. */
async function waitForPresentAtGen(page: Page, gen: number, budgetMs: number): Promise<void> {
  await expect
    .poll(
      () =>
        page.evaluate(
          (g) =>
            (
              (window as unknown as { __atermPresentLog?: PresentSample[] }).__atermPresentLog ?? []
            ).some((e) => e.gen >= g),
          gen
        ),
      { timeout: budgetMs, message: `No present reached content gen ${gen}` }
    )
    .toBe(true)
}

function percentile(sortedAsc: number[], q: number): number {
  const n = sortedAsc.length
  if (n === 0) {
    return 0
  }
  // Nearest-rank on the sorted samples (matches the existing latency summarize()).
  return sortedAsc[Math.min(n - 1, Math.floor(q * n))] ?? 0
}

test.describe('Keystroke-to-present latency (aterm shipped renderer)', () => {
  test('measures real keydown->present latency per keystroke', async ({
    orcaPage,
    testRepoPath
  }, testInfo) => {
    test.setTimeout(180_000)
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)
    await ensureTerminalVisible(orcaPage)
    await waitForActiveTerminalManager(orcaPage, 30_000)

    const ptyId = await waitForActivePanePtyId(orcaPage)
    const runId = randomUUID()
    const scriptPath = path.join(testRepoPath, `.orca-present-latency-${runId}.mjs`)
    writeFileSync(scriptPath, interactivePromptScript(runId))
    let commandSent = false
    try {
      await sendToTerminal(orcaPage, ptyId, `node ${JSON.stringify(scriptPath)}\r`)
      commandSent = true
      await waitForTerminalOutput(orcaPage, `LAT_READY_${runId}`, 10_000)
      await focusActiveTerminalInput(orcaPage)
      await armLatencyProbe(orcaPage)

      // Per-keystroke: type one char, wait for its echo in the grid, read the
      // content gen G at that moment, then ensure a present has reached G. All
      // correlation math runs at the end from the two renderer-clock logs.
      const genByKeystroke: number[] = []
      const typedChars: string[] = []
      for (let i = 0; i < TOTAL_KEYSTROKES; i++) {
        const seq = i + 1
        const char = ALPHABET[i % ALPHABET.length]
        typedChars.push(char)
        const marker = `LAT_KEY_${runId}_${seq}`
        await orcaPage.keyboard.type(char)
        await waitForTerminalOutput(orcaPage, marker, PER_KEYSTROKE_BUDGET_MS)
        const gen = await readContentGen(orcaPage)
        await waitForPresentAtGen(orcaPage, gen, PER_KEYSTROKE_BUDGET_MS)
        genByKeystroke.push(gen)
      }

      const presentLog = await readPresentLog(orcaPage)
      const keydownLogRaw = await readKeydownLog(orcaPage)
      // Only single-character keydowns are our typed keystrokes (no modifiers).
      const keydownLog = keydownLogRaw.filter((k) => k.key.length === 1)

      expect(keydownLog.length).toBeGreaterThanOrEqual(TOTAL_KEYSTROKES)
      expect(presentLog.length).toBeGreaterThan(0)

      // Correlate keystroke i -> T0 (i-th typed keydown) and T1 (first present at
      // gen >= G_i). gen is strictly monotonic, so the first qualifying present is
      // the frame that first put keystroke i's echo on glass.
      const samplesMs: number[] = []
      const perSample: { i: number; char: string; gen: number; latencyMs: number }[] = []
      for (let i = 0; i < TOTAL_KEYSTROKES; i++) {
        const keydown = keydownLog[i]
        // Sanity: index alignment must match the char we typed.
        if (!keydown || keydown.key !== typedChars[i]) {
          throw new Error(
            `keydown/typed mismatch at ${i}: got ${keydown?.key ?? 'none'} want ${typedChars[i]}`
          )
        }
        const g = genByKeystroke[i]
        const present = presentLog.find((e) => e.gen >= g && e.t >= keydown.t)
        if (!present) {
          throw new Error(`No present with gen >= ${g} after keydown ${i}`)
        }
        const latencyMs = present.t - keydown.t
        perSample.push({ i, char: typedChars[i], gen: g, latencyMs })
        if (i >= WARMUP_KEYSTROKES) {
          samplesMs.push(latencyMs)
        }
      }

      const sorted = [...samplesMs].sort((a, b) => a - b)
      const n = sorted.length
      const mean = n > 0 ? sorted.reduce((acc, v) => acc + v, 0) / n : 0
      const result = {
        built: true,
        spec: 'tests/e2e/terminal-keystroke-present-latency.spec.ts',
        renderPath: 'cpu-in-process',
        correlation: 'gen-exact' as const,
        n,
        warmup: WARMUP_KEYSTROKES,
        totalKeystrokes: TOTAL_KEYSTROKES,
        median_ms: percentile(sorted, 0.5),
        p50_ms: percentile(sorted, 0.5),
        p90_ms: percentile(sorted, 0.9),
        p99_ms: percentile(sorted, 0.99),
        mean_ms: mean,
        min_ms: sorted[0] ?? 0,
        max_ms: sorted[n - 1] ?? 0,
        samples_ms: sorted.map((v) => Number(v.toFixed(3))),
        perSample
      }

      const outPath =
        process.env.ORCA_LATENCY_OUT ?? path.join(testInfo.outputDir, 'latency_result.json')
      mkdirSync(path.dirname(outPath), { recursive: true })
      writeFileSync(outPath, `${JSON.stringify(result, null, 2)}\n`)

      testInfo.annotations.push({
        type: 'keystroke-present-latency',
        description: `median=${result.median_ms.toFixed(1)}ms p90=${result.p90_ms.toFixed(
          1
        )}ms p99=${result.p99_ms.toFixed(1)}ms mean=${result.mean_ms.toFixed(
          1
        )}ms min=${result.min_ms.toFixed(1)}ms n=${n} (gen-exact, cpu-in-process) -> ${outPath}`
      })

      // Guard rails only (this is a measurement, not a tight budget): the shipped
      // renderer must stay well under the coarse guard's 250ms median.
      expect(result.median_ms).toBeLessThan(250)
      expect(n).toBe(TOTAL_KEYSTROKES - WARMUP_KEYSTROKES)
    } finally {
      if (commandSent) {
        await sendToTerminal(orcaPage, ptyId, '\x03').catch(() => undefined)
      }
      rmSync(scriptPath, { force: true })
    }
  })
})
