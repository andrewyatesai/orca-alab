import { randomUUID } from 'node:crypto'
import { rmSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import type { ElectronApplication, Locator, Page, TestInfo } from '@stablyai/playwright-test'
import { test, expect } from './helpers/orca-app'
import { sendToTerminal } from './helpers/terminal'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import {
  ensureActiveWorktreePaneLoad,
  getTerminalContentForPtyId
} from './artificial-opencode-pane-interactions'
import { nodeTerminalCommand } from './terminal-node-command'
import { buildFreshShellProbeInputSequence } from './terminal-probe-input-sequence'
import { waitForPtyShellEcho } from './terminal-pty-readiness'
import { stripSerializedControlSequences } from './terminal-serialized-text'

// Coordinator v0 acceptance gates (docs/rust-migration/coordinator-v0-design.md
// §"Acceptance gates (measured)") as executable MEASUREMENTS, following the
// aterm-echo-latency.spec.ts precedent: every gate records its measured timing
// to console (RESULT_JSON) and testInfo.attach, asserts only a generous
// PATHOLOGY bound (a number near it means broken, not slow), and reports
// whether the design gate itself was met without failing the test on it.
//
//   Gate 1 — time-to-first-success: fresh window → reading a live session (<5s design)
//   Gate 2 — recovery: kill window mid-session, reopen → every session back,
//            zero bytes lost via daemon snapshot+scrollback hydration (<3s design)
//   Gate 3 — attention correctness: an agent hitting a prompt appears in the
//            queue (<2s design; note the feed's 3s getForegroundProcess poll
//            floor makes this gate an honest architecture probe, not a formality)
//
// The coordinator window has no test hook: these specs drive the REAL entry
// point — the Window ▸ Coordinator application-menu item — via the main
// process (feature-wall.spec.ts precedent), so the measured spans include
// exactly what a user's click includes.
//
// Rendering assumption (spec-authoring time): tiles and the focused view are
// text-tail DOM (session-tiles.tsx / focused-session-view.tsx). All reads go
// through waitForCoordinatorBodyText / sessionTileByMarker below; if tiles
// become aterm-rendered, swap those two seams to the tile's logical-text
// probe — the gate protocol itself does not change.
//
// Run the real measurement (single worker so app cold-starts don't contend):
//   npx playwright test --config tests/playwright.config.ts \
//     tests/e2e/coordinator-acceptance.spec.ts --project electron-headless --workers=1

// Design gates — REPORTED per run, never asserted.
const GATE1_DESIGN_MS = 5_000
const GATE2_DESIGN_MS = 3_000
const GATE3_DESIGN_MS = 2_000
// Pathology bounds — asserted. Sized to absorb CI cold-start noise while still
// catching a broken pipeline (no hydration, no fan-out, no foreground poll).
const GATE1_PATHOLOGY_MS = 30_000
const GATE2_PATHOLOGY_MS = 30_000
const GATE3_PATHOLOGY_MS = 20_000
// In-page DOM polling for timed conditions: cheap enough at 50ms, and the
// quantization error stays well under every gate's granularity.
const DOM_POLL_MS = 50
// Node-side locator polling (queue entries) and owner-terminal marker polling.
const LOCATOR_POLL_MS = 30
const OWNER_POLL_MS = 25

// Tiles are the Card `role="button"` divs in the grid; attention-queue entries
// are real <button> elements. That tag difference is the disambiguator all
// selectors below rely on.
const SESSION_TILE_SELECTOR = 'div[role="button"]'

function seqToken(markerId: string, index: number): string {
  // Zero-padded so no token is a prefix of another (SEQ_x_1 vs SEQ_x_10).
  return `SEQ_${markerId}_${String(index).padStart(3, '0')}`
}

function seqPrintScript(markerId: string): string {
  return `
const start = Number(process.argv[2])
const end = Number(process.argv[3])
for (let index = start; index <= end; index += 1) {
  process.stdout.write('SEQ_${markerId}_' + String(index).padStart(3, '0') + '\\r\\n')
}
`
}

// A minimal "agent": holds the PTY foreground as a non-shell process (status
// Working), then on any stdin byte prints PROMPT_HIT and exits back to the
// shell prompt — the exact transition gate 3 times.
function promptSimScript(markerId: string): string {
  return `
process.stdin.setEncoding('utf8')
if (process.stdin.isTTY) process.stdin.setRawMode(true)
process.stdin.resume()
process.stdout.write('AGENT_BUSY_${markerId}\\r\\n')
process.stdin.on('data', () => {
  process.stdout.write('PROMPT_HIT_${markerId}\\r\\n')
  process.exit(0)
})
`
}

// Base64-encode the marker inside the command so seeing the plain marker
// proves the shell EXECUTED it — the typed-command echo never contains it
// (waitForPtyShellEcho's documented convention).
function encodedMarkerEchoCommand(marker: string): string {
  const encoded = Buffer.from(marker, 'utf8').toString('base64')
  return `${nodeTerminalCommand([
    '-e',
    `console.log(Buffer.from('${encoded}', 'base64').toString('utf8'))`
  ])}\r`
}

async function launchTerminalScript(
  page: Page,
  ptyId: string,
  scriptPath: string,
  args: readonly string[]
): Promise<void> {
  for (const input of buildFreshShellProbeInputSequence(
    `${nodeTerminalCommand([scriptPath, ...args])}\r`
  )) {
    await sendToTerminal(page, ptyId, input)
  }
}

// The coordinator's only entry point is the Window ▸ Coordinator menu item —
// clicking it from the main process drives the real openCoordinatorWindow path.
async function openCoordinatorViaWindowMenu(electronApp: ElectronApplication): Promise<Page> {
  // Register before the click so the 'window' event cannot be missed.
  const windowPromise = electronApp.waitForEvent('window', { timeout: 30_000 })
  await electronApp.evaluate(({ BrowserWindow, Menu }) => {
    const coordinatorItem = Menu.getApplicationMenu()
      ?.items.find((item) => item.label === 'Window')
      ?.submenu?.items.find((item) => item.label === 'Coordinator')
    if (!coordinatorItem) {
      throw new Error('Coordinator menu item was not registered under the Window menu')
    }
    const focusedWindow = BrowserWindow.getAllWindows()[0]
    coordinatorItem.click(coordinatorItem, focusedWindow, {
      triggeredByAccelerator: false,
      shiftKey: false,
      metaKey: false,
      ctrlKey: false,
      altKey: false
    } as Electron.KeyboardEvent)
  })
  const coordinatorPage = await windowPromise
  await coordinatorPage.waitForLoadState('domcontentloaded')
  if (!coordinatorPage.url().includes('coordinator')) {
    throw new Error(`expected the coordinator window, got ${coordinatorPage.url()}`)
  }
  return coordinatorPage
}

async function destroyCoordinatorWindow(
  electronApp: ElectronApplication,
  coordinatorPage: Page
): Promise<void> {
  await electronApp.evaluate(({ BrowserWindow }) => {
    const coordinator = BrowserWindow.getAllWindows().find((candidate) =>
      candidate.webContents.getURL().includes('coordinator')
    )
    if (!coordinator) {
      throw new Error('no coordinator window found to destroy')
    }
    // destroy(), not close(): the gate says KILL the window mid-session — no
    // graceful unload; the daemon must carry every byte across the gap.
    coordinator.destroy()
  })
  await expect
    .poll(() => coordinatorPage.isClosed(), {
      timeout: 10_000,
      message: 'coordinator page should close after BrowserWindow.destroy()'
    })
    .toBe(true)
}

// The one coordinator read seam: what the DOM actually shows the user
// (innerText — layout-honest), never engine internals. If tiles become
// aterm-rendered canvases, replace this body with the tile logical-text probe.
async function waitForCoordinatorBodyText(
  page: Page,
  requiredFragments: readonly string[],
  timeoutMs: number
): Promise<void> {
  try {
    await page.waitForFunction(
      (fragments) => {
        const text = document.body.innerText
        return fragments.every((fragment) => text.includes(fragment))
      },
      [...requiredFragments],
      { timeout: timeoutMs, polling: DOM_POLL_MS }
    )
  } catch (error) {
    const tail = await page
      .evaluate(() => document.body.innerText.slice(-800))
      .catch(() => '<coordinator DOM unreadable>')
    throw new Error(
      `coordinator DOM never showed ${JSON.stringify(requiredFragments)} within ${timeoutMs}ms; body tail: ${JSON.stringify(tail)}`,
      { cause: error }
    )
  }
}

function sessionTileByMarker(page: Page, marker: string): Locator {
  return page.locator(SESSION_TILE_SELECTOR).filter({ hasText: marker })
}

// A queue entry is a real <button> carrying the session title AND the
// "Needs you" chip (done/failed sessions also queue, with different chips).
function attentionQueueEntryByTitle(page: Page, title: string): Locator {
  return page.locator('button').filter({ hasText: title }).filter({ hasText: 'Needs you' })
}

async function readSessionTileTitle(tile: Locator): Promise<string> {
  // CardTitle mirrors the full session title into its `title` attribute — the
  // stable correlation key between a grid tile and its queue entry.
  const title = await tile.locator('[title]').first().getAttribute('title')
  if (!title) {
    throw new Error('session tile exposes no title attribute to correlate with the queue')
  }
  return title
}

async function measureElapsedUntil(
  predicate: () => Promise<boolean>,
  timeoutMs: number,
  description: string
): Promise<number> {
  const startedAt = performance.now()
  for (;;) {
    if (await predicate()) {
      return performance.now() - startedAt
    }
    if (performance.now() - startedAt > timeoutMs) {
      throw new Error(`timed out after ${timeoutMs}ms waiting for ${description}`)
    }
    await new Promise((resolve) => setTimeout(resolve, LOCATOR_POLL_MS))
  }
}

async function waitForOwnerTerminalMarker(
  page: Page,
  ptyId: string,
  marker: string,
  timeoutMs: number
): Promise<void> {
  const startedAt = performance.now()
  for (;;) {
    // Serialize can split a wrapped marker with cursor-move controls; strip
    // them so the match is width-independent (waitForPtyShellEcho convention).
    const content = stripSerializedControlSequences(await getTerminalContentForPtyId(page, ptyId))
    if (content.includes(marker)) {
      return
    }
    if (performance.now() - startedAt > timeoutMs) {
      throw new Error(
        `owner terminal ${ptyId} never showed ${marker} within ${timeoutMs}ms (tail: ${JSON.stringify(content.slice(-200))})`
      )
    }
    await page.waitForTimeout(OWNER_POLL_MS)
  }
}

type AcceptanceSessionContext = { ptyId: string; markerId: string }

async function setUpLiveSession(orcaPage: Page): Promise<AcceptanceSessionContext> {
  await waitForSessionReady(orcaPage)
  await waitForActiveWorktree(orcaPage)
  const [pane] = await ensureActiveWorktreePaneLoad(orcaPage, 1)
  if (!pane?.ptyId) {
    throw new Error('expected a PTY-bound terminal pane for the coordinator acceptance gates')
  }
  // Requires the daemon PTY path: if daemon startup fell back to local PTYs,
  // the coordinator lists nothing and the gate fails honestly downstream.
  await waitForPtyShellEcho(orcaPage, pane.ptyId, 30_000)
  return { ptyId: pane.ptyId, markerId: randomUUID().slice(0, 8) }
}

type GateReport = {
  gate: string
  designGateMs: number
  designGateMet: boolean
  measured: Record<string, number>
  detail?: Record<string, unknown>
}

async function reportGate(testInfo: TestInfo, report: GateReport): Promise<void> {
  const summary = {
    ...report,
    measured: Object.fromEntries(
      Object.entries(report.measured).map(([key, value]) => [key, Number(value.toFixed(1))])
    ),
    platform: process.platform
  }
  const measuredText = Object.entries(summary.measured)
    .map(([key, value]) => `${key}=${value}ms`)
    .join(' | ')
  const line = `[coordinator-acceptance] ${report.gate}: ${measuredText} (design gate ${report.designGateMs}ms → ${report.designGateMet ? 'MET' : 'MISSED'}; pathology bound is the only assert)`
  // eslint-disable-next-line no-console
  console.log(`\n${line}\n`)
  // eslint-disable-next-line no-console
  console.log(`[coordinator-acceptance] RESULT_JSON ${JSON.stringify(summary)}`)
  testInfo.annotations.push({
    type: 'coordinator-acceptance',
    description: line
  })
  await testInfo.attach(`coordinator-${report.gate}.json`, {
    body: JSON.stringify(summary, null, 2),
    contentType: 'application/json'
  })
}

test.describe('coordinator v0 acceptance gates @coordinator-acceptance', () => {
  test('gate 1: fresh coordinator window reads a live session (time-to-first-success)', async ({
    electronApp,
    orcaPage
  }, testInfo) => {
    test.setTimeout(240_000)
    const { ptyId, markerId } = await setUpLiveSession(orcaPage)
    const liveMarker = `GATE1_LIVE_${markerId}`

    // The session is live and has produced identifiable output BEFORE the
    // window opens — the gate measures the coordinator, not the shell.
    await sendToTerminal(orcaPage, ptyId, encodedMarkerEchoCommand(liveMarker))
    await waitForOwnerTerminalMarker(orcaPage, ptyId, liveMarker, 20_000)

    // t0 = the user's menu click. The span covers window creation, renderer
    // load, tunnel+daemon connect, listSessions, subscribe hydration, render.
    const openStartedAt = performance.now()
    const coordinatorPage = await openCoordinatorViaWindowMenu(electronApp)
    await waitForCoordinatorBodyText(coordinatorPage, ['Connected', liveMarker], GATE1_PATHOLOGY_MS)
    const openToSessionReadableMs = performance.now() - openStartedAt

    const tile = sessionTileByMarker(coordinatorPage, liveMarker).first()
    await expect(tile, 'session tile with live output is visible').toBeVisible()
    await expect(tile, 'tile carries a plain-language status chip').toContainText(
      /Working|Needs you|Done|Failed/
    )
    // Correct status: an idle shell prompt IS the attention state; the feed
    // needs one getForegroundProcess tick (3s poll) to observe it.
    await expect(tile, 'idle shell surfaces as Needs you').toContainText('Needs you', {
      timeout: 20_000
    })

    await tile.click()
    await waitForCoordinatorBodyText(coordinatorPage, ['Read-only view', liveMarker], 15_000)
    const openToFocusedReadableMs = performance.now() - openStartedAt

    // Live-update path (subscriber fan-out, not hydration): output produced
    // NOW must reach the already-open focused view. Includes ~100-300ms of
    // shell exec, so it is an upper bound on the fan-out latency itself.
    const updateMarker = `GATE1_UPDATE_${markerId}`
    const liveUpdateStartedAt = performance.now()
    await sendToTerminal(orcaPage, ptyId, encodedMarkerEchoCommand(updateMarker))
    await waitForCoordinatorBodyText(coordinatorPage, [updateMarker], 15_000)
    const liveUpdateReflectedMs = performance.now() - liveUpdateStartedAt

    await reportGate(testInfo, {
      gate: 'gate1-time-to-first-success',
      designGateMs: GATE1_DESIGN_MS,
      designGateMet: openToSessionReadableMs < GATE1_DESIGN_MS,
      measured: {
        openToSessionReadableMs,
        openToFocusedReadableMs,
        liveUpdateReflectedMs
      },
      detail: {
        definition:
          'menu click → Connected badge + live session output readable in the tile preview'
      }
    })

    expect(openToSessionReadableMs, 'open→readable is positive').toBeGreaterThan(0)
    expect(
      openToSessionReadableMs,
      `open→readable under ${GATE1_PATHOLOGY_MS}ms (pathology bound)`
    ).toBeLessThan(GATE1_PATHOLOGY_MS)
    expect(Number.isFinite(openToFocusedReadableMs), 'open→focused finite').toBe(true)
    expect(Number.isFinite(liveUpdateReflectedMs), 'live-update finite').toBe(true)
  })

  test('gate 2: kill and reopen the coordinator window loses zero bytes (recovery)', async ({
    electronApp,
    orcaPage,
    testRepoPath
  }, testInfo) => {
    test.setTimeout(240_000)
    const { ptyId, markerId } = await setUpLiveSession(orcaPage)
    const seqScriptPath = path.join(testRepoPath, `.orca-coordinator-seq-${randomUUID()}.mjs`)
    writeFileSync(seqScriptPath, seqPrintScript(markerId))

    try {
      await launchTerminalScript(orcaPage, ptyId, seqScriptPath, ['1', '10'])
      await waitForOwnerTerminalMarker(orcaPage, ptyId, seqToken(markerId, 10), 20_000)

      const coordinatorPage = await openCoordinatorViaWindowMenu(electronApp)
      await waitForCoordinatorBodyText(
        coordinatorPage,
        ['Connected', seqToken(markerId, 10)],
        30_000
      )
      const tilesBefore = await coordinatorPage.locator(SESSION_TILE_SELECTOR).count()
      expect(tilesBefore, 'live session tile present before the kill').toBeGreaterThanOrEqual(1)

      await destroyCoordinatorWindow(electronApp, coordinatorPage)

      // Bytes produced while NO coordinator window exists can only reach the
      // reopened window via daemon snapshot+scrollback hydration — this is the
      // substance of the zero-bytes-lost claim.
      await launchTerminalScript(orcaPage, ptyId, seqScriptPath, ['11', '20'])
      await waitForOwnerTerminalMarker(orcaPage, ptyId, seqToken(markerId, 20), 20_000)

      const reopenStartedAt = performance.now()
      const reopenedPage = await openCoordinatorViaWindowMenu(electronApp)
      // Sessions are "back" when the grid shows our session WITH the newest
      // dark-period output — presence without the tail would be a hollow pass.
      await waitForCoordinatorBodyText(
        reopenedPage,
        ['Connected', seqToken(markerId, 20)],
        GATE2_PATHOLOGY_MS
      )
      const reopenToSessionsBackMs = performance.now() - reopenStartedAt

      await expect
        .poll(() => reopenedPage.locator(SESSION_TILE_SELECTOR).count(), {
          timeout: 15_000,
          message: 'every session tile returns after reopen'
        })
        .toBe(tilesBefore)

      await sessionTileByMarker(reopenedPage, seqToken(markerId, 20)).first().click()
      await waitForCoordinatorBodyText(reopenedPage, ['Read-only view'], 15_000)
      const focusedText = await reopenedPage.evaluate(() => document.body.innerText)

      // Zero-loss: all 20 sequence tokens — 10 pre-kill, 10 while dark — are
      // present in stream order across the hydration boundary.
      let lastIndex = -1
      for (let index = 1; index <= 20; index += 1) {
        const token = seqToken(markerId, index)
        const foundAt = focusedText.indexOf(token)
        expect(foundAt, `${token} survived the window kill (zero-loss)`).toBeGreaterThan(-1)
        expect(foundAt, `${token} appears in stream order`).toBeGreaterThan(lastIndex)
        lastIndex = foundAt
      }

      await reportGate(testInfo, {
        gate: 'gate2-recovery-zero-loss',
        designGateMs: GATE2_DESIGN_MS,
        designGateMet: reopenToSessionsBackMs < GATE2_DESIGN_MS,
        measured: { reopenToSessionsBackMs },
        detail: {
          definition:
            'menu reopen after BrowserWindow.destroy → grid shows the session with its newest dark-period output',
          zeroLoss: {
            tokensExpected: 20,
            tokensFound: 20,
            streamOrderPreserved: true
          },
          tiles: { before: tilesBefore, after: tilesBefore }
        }
      })

      expect(
        reopenToSessionsBackMs,
        `reopen→sessions-back under ${GATE2_PATHOLOGY_MS}ms (pathology bound)`
      ).toBeLessThan(GATE2_PATHOLOGY_MS)
    } finally {
      rmSync(seqScriptPath, { force: true })
    }
  })

  test('gate 3: an agent hitting a prompt appears in the attention queue', async ({
    electronApp,
    orcaPage,
    testRepoPath
  }, testInfo) => {
    test.setTimeout(240_000)
    const { ptyId, markerId } = await setUpLiveSession(orcaPage)
    const promptScriptPath = path.join(testRepoPath, `.orca-coordinator-prompt-${randomUUID()}.mjs`)
    writeFileSync(promptScriptPath, promptSimScript(markerId))
    const tagMarker = `ATTN_TAG_${markerId}`
    let scriptLaunched = false

    try {
      await sendToTerminal(orcaPage, ptyId, encodedMarkerEchoCommand(tagMarker))
      await waitForOwnerTerminalMarker(orcaPage, ptyId, tagMarker, 20_000)

      const coordinatorPage = await openCoordinatorViaWindowMenu(electronApp)
      await waitForCoordinatorBodyText(coordinatorPage, ['Connected', tagMarker], 30_000)
      const tile = sessionTileByMarker(coordinatorPage, tagMarker).first()
      await expect(tile, 'tagged session tile is visible').toBeVisible()
      const title = await readSessionTileTitle(tile)

      // Baseline: the idle shell prompt must already queue as needs-you — this
      // proves the foreground-process pipeline works before timing a transition.
      const baselineNeedsYouMs = await measureElapsedUntil(
        async () => (await attentionQueueEntryByTitle(coordinatorPage, title).count()) > 0,
        30_000,
        'baseline needs-you queue entry for the idle shell prompt'
      )

      await launchTerminalScript(orcaPage, ptyId, promptScriptPath, [])
      scriptLaunched = true
      await waitForOwnerTerminalMarker(orcaPage, ptyId, `AGENT_BUSY_${markerId}`, 20_000)
      // The busy (non-shell) foreground process must CLEAR the attention state,
      // otherwise the timed reappearance below would measure nothing.
      await expect(
        attentionQueueEntryByTitle(coordinatorPage, title),
        'busy agent leaves the attention queue'
      ).toHaveCount(0, { timeout: 20_000 })
      await expect(tile, 'busy agent reads as Working').toContainText('Working')

      // The poke makes the "agent" print PROMPT_HIT and exit to the shell
      // prompt. t0 = that output landing in the owner window — the closest
      // observable to "the agent hit a prompt". The measured span then covers
      // the daemon foreground poll (3s cadence floor), status derivation, and
      // the queue render.
      await sendToTerminal(orcaPage, ptyId, 'g')
      await waitForOwnerTerminalMarker(orcaPage, ptyId, `PROMPT_HIT_${markerId}`, 15_000)
      const promptToQueueMs = await measureElapsedUntil(
        async () => (await attentionQueueEntryByTitle(coordinatorPage, title).count()) > 0,
        GATE3_PATHOLOGY_MS,
        'needs-you queue entry after the agent hit its prompt'
      )
      await expect(tile, 'prompted agent reads as Needs you').toContainText('Needs you')

      await reportGate(testInfo, {
        gate: 'gate3-attention-latency',
        designGateMs: GATE3_DESIGN_MS,
        designGateMet: promptToQueueMs < GATE3_DESIGN_MS,
        measured: { promptToQueueMs, baselineNeedsYouMs },
        detail: {
          definition:
            'agent prints its prompt and exits to the shell → session appears as Needs you in the queue',
          feedPollFloorMs: 3000,
          note: 'the coordinator polls getForegroundProcess every 3s, so the 2s design gate cannot be met deterministically without a push-based foreground signal'
        }
      })

      expect(promptToQueueMs, 'prompt→queue is positive').toBeGreaterThan(0)
      expect(
        promptToQueueMs,
        `prompt→queue under ${GATE3_PATHOLOGY_MS}ms (pathology bound)`
      ).toBeLessThan(GATE3_PATHOLOGY_MS)
    } finally {
      if (scriptLaunched) {
        // If the prompt-sim is still in the foreground, any byte exits it.
        await sendToTerminal(orcaPage, ptyId, '\x03').catch(() => undefined)
      }
      rmSync(promptScriptPath, { force: true })
    }
  })
})
