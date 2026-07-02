import { test, expect } from './helpers/orca-app'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveAtermController } from './helpers/aterm-controller'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'

// HONEST per-pane MEMORY measurement, run in the real Electron renderer — answers
// the adversarial review's "a whole VT engine per pane is bloated, and it's
// unmeasured". window.__atermMemoryBench builds several LIVE aterm engines (each
// fed glyph-diverse scrollback + rendered) and reports the wasm-heap growth per
// pane. It is a MEASUREMENT (loose sanity asserts), not a gate; the number is logged.
//
// Key honesty points the bench encodes: (1) the big OS fallback fonts (CJK ~100MB,
// emoji ~180MB) are interned to ONE shared copy across panes (aterm-render intern +
// its native unit test), so they're a one-time cost, NOT per-pane — excluded here.
// (2) It measures the CPU-fallback engine, whose wasm heap holds the RGBA
// framebuffer — so the figure is the UPPER BOUND; the shipped GPU default keeps the
// framebuffer in GPU textures (not wasm), so it's lighter per pane. (3) The kept
// xterm shim's per-pane buffer is a SEPARATE JS-heap cost (Phase-3 removal target),
// not in the wasm heap measured here.

type MemBenchResult = {
  panes: number
  scrollbackLines: number
  cols: number
  rows: number
  bytesPerPane: number
  kbPerPane: number
  totalHeapBytes: number
}
type MemBenchProbe = {
  __atermMemoryBench?: (
    cols: number,
    rows: number,
    scrollbackLines: number,
    panes: number
  ) => Promise<MemBenchResult>
}
type WorkerRenderProbe = {
  __atermWorkerRender?: boolean
  __atermWorkerRenderState?: unknown
}

test.describe('aterm per-pane memory @aterm-memory', () => {
  test('measures the wasm-heap footprint per aterm pane (fonts deduped/excluded)', async ({
    orcaPage
  }, testInfo) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)
    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()
    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas).toBeAttached({ timeout: 20_000 })
    await waitForActivePanePtyId(orcaPage)
    await waitForActiveAtermController(orcaPage)

    await expect
      .poll(
        async () =>
          orcaPage.evaluate(
            () => typeof (window as unknown as MemBenchProbe).__atermMemoryBench === 'function'
          ),
        { timeout: 30_000, message: 'aterm memory bench hook should be ready' }
      )
      .toBe(true)

    const PANES = 4
    const SCROLLBACK = 1000
    const result = (await orcaPage.evaluate(
      ({ panes, scrollback }) => {
        const fn = (window as unknown as MemBenchProbe).__atermMemoryBench
        return fn ? fn(120, 40, scrollback, panes) : null
      },
      { panes: PANES, scrollback: SCROLLBACK }
    )) as MemBenchResult | null

    expect(result, 'memory bench returned a result').not.toBeNull()
    const r = result as MemBenchResult

    const line =
      `[aterm-memory] ${r.panes} live panes @ ${r.cols}x${r.rows}, ${r.scrollbackLines} ` +
      `scrollback lines each → ${r.kbPerPane} KB/pane (CPU-engine upper bound — wasm heap: ` +
      `grid + scrollback + framebuffer + atlas; GPU default lighter, framebuffer in GPU ` +
      `textures; OS fallback fonts deduped to one shared copy, excluded). ` +
      `total wasm heap ${(r.totalHeapBytes / (1024 * 1024)).toFixed(1)} MB.`
    // eslint-disable-next-line no-console
    console.log(`\n${line}\n`)
    testInfo.annotations.push({ type: 'aterm-memory', description: line })

    // Loose sanity: a real, positive per-pane footprint that isn't absurd. A 120x40
    // grid + 1000 scrollback lines + a device-pixel framebuffer is on the order of
    // a few MB; assert > 0 and < 64 MB/pane (a pathological number would mean the
    // font dedup or scrollback bound is broken).
    expect(r.bytesPerPane, 'per-pane wasm footprint is positive').toBeGreaterThan(0)
    expect(
      r.bytesPerPane,
      'per-pane wasm footprint is not pathological (font dedup + scrollback bound hold)'
    ).toBeLessThan(64 * 1024 * 1024)
  })

  // REGRESSION GATE for the shared render worker (audit E1): the DEFAULT worker path
  // must not pay the font payload per pane. The per-pane-worker architecture shipped
  // the primary face + the CJK/script fallback chain + the colour-emoji face (~tens
  // to hundreds of MB — Apple Color Emoji alone is ~190MB) into EVERY pane's worker
  // and interned another copy in every pane's own wasm instance, so each additional
  // pane cost hundreds of MB of renderer RSS. With ONE shared worker the fonts are
  // sent once and every engine interns against the same wasm-module registry, so the
  // marginal cost of panes 2..N is just the engine (grid + scrollback + framebuffer),
  // a few MB. The 48MB/pane threshold is far above the shared-worker marginal cost
  // (noise headroom) and far below any per-pane font payload — the old architecture
  // FAILS this on every supported platform.
  test('worker-path panes 2..N do not re-pay the font payload (shared render worker)', async ({
    orcaPage,
    electronApp
  }, testInfo) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)
    // Opt INTO the worker render path (the e2e suite defaults it off; production is on).
    await orcaPage.evaluate(() => {
      ;(window as unknown as WorkerRenderProbe).__atermWorkerRender = true
    })

    const canvasCount = (): Promise<number> =>
      orcaPage.locator('[data-testid="aterm-canvas"]').count()

    const openWorkerPane = async (nth: number): Promise<void> => {
      // Relative count: session-restore/agent panes can mount their own canvases late,
      // so only require that THIS open added one (absolute counts are racy).
      const before = await canvasCount()
      // Clear the last worker STATE so readiness below provably comes from THIS pane.
      // Open via the store action (the same one the "+ → New Terminal" menu drives):
      // the UI affordance is covered elsewhere, and repeated dropdown opens are flaky.
      await orcaPage.evaluate(() => {
        ;(window as unknown as WorkerRenderProbe).__atermWorkerRenderState = undefined
        const store = window.__store
        if (!store) {
          throw new Error('Store unavailable')
        }
        const worktreeId = store.getState().activeWorktreeId
        if (!worktreeId) {
          throw new Error('No active worktree')
        }
        const terminal = store.getState().createTab(worktreeId)
        store.getState().setActiveTab(terminal.id)
        store.getState().setActiveTabType('terminal')
      })
      await expect
        .poll(canvasCount, { timeout: 20_000, message: `terminal pane ${nth} should mount` })
        .toBeGreaterThan(before)
      await waitForActivePanePtyId(orcaPage)
      await expect
        .poll(
          async () =>
            orcaPage.evaluate(() =>
              Boolean((window as unknown as WorkerRenderProbe).__atermWorkerRenderState)
            ),
          { timeout: 30_000, message: `pane ${nth} should render via the worker path` }
        )
        .toBe(true)
    }

    // Renderer-process RSS: min of several samples — the floor is stabler than an
    // instantaneous reading (GC of transient postMessage/transfer copies).
    const rendererWorkingSetBytes = async (): Promise<number> => {
      let min = Number.POSITIVE_INFINITY
      for (let i = 0; i < 6; i++) {
        const kb = await electronApp.evaluate(({ BrowserWindow, app }) => {
          const wc = BrowserWindow.getAllWindows()[0]?.webContents
          if (!wc) {
            return -1
          }
          const pid = wc.getOSProcessId()
          const metric = app.getAppMetrics().find((m) => m.pid === pid)
          return metric ? metric.memory.workingSetSize : -1
        })
        if (kb > 0) {
          min = Math.min(min, kb * 1024)
        }
        await orcaPage.waitForTimeout(400)
      }
      expect(min, 'renderer process memory should be measurable').toBeLessThan(
        Number.POSITIVE_INFINITY
      )
      return min
    }

    // Pane 1 pays the one-time costs (worker spawn, wasm modules, the full font
    // payload); measure AFTER it so the gate isolates the marginal per-pane cost.
    await openWorkerPane(1)
    await orcaPage.waitForTimeout(2000) // let the first-pane boot allocations settle
    const afterFirst = await rendererWorkingSetBytes()

    await openWorkerPane(2)
    await openWorkerPane(3)
    await orcaPage.waitForTimeout(2000)
    const afterThird = await rendererWorkingSetBytes()

    const marginalMB = (afterThird - afterFirst) / 2 / (1024 * 1024)
    const line =
      `[aterm-worker-memory] renderer RSS after pane 1: ${(afterFirst / (1024 * 1024)).toFixed(1)} MB; ` +
      `after pane 3: ${(afterThird / (1024 * 1024)).toFixed(1)} MB → marginal ` +
      `${marginalMB.toFixed(1)} MB/pane for worker panes 2-3 (gate: < 48 MB/pane).`
    // eslint-disable-next-line no-console
    console.log(`\n${line}\n`)
    testInfo.annotations.push({ type: 'aterm-worker-memory', description: line })

    expect(
      marginalMB,
      'marginal per-pane cost must exclude the font payload (shared worker + interned fonts)'
    ).toBeLessThan(48)
  })
})
