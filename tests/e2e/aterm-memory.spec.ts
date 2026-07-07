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

type OrcaPage = Parameters<Parameters<typeof test>[2]>[0]['orcaPage']

// Worker-path helpers shared by the shared-worker gates: open a pane through the
// store (dropdown UI covered elsewhere; repeated opens are flaky) and read the
// worker's wasm linear-memory size off the per-frame state message (module-wide,
// so any pane's message reports it; wasm memory only grows and holds the interned
// fonts — renderer RSS cannot resolve that signal against GC noise).
function workerPaneProbes(orcaPage: OrcaPage): {
  openWorkerPane: (nth: number) => Promise<void>
  workerWasmHeapBytes: () => Promise<number>
} {
  const canvasCount = (): Promise<number> =>
    orcaPage.locator('[data-testid="aterm-canvas"]').count()

  const openWorkerPane = async (nth: number): Promise<void> => {
    // Relative count: session-restore/agent panes can mount their own canvases late,
    // so only require that THIS open added one (absolute counts are racy).
    const before = await canvasCount()
    // Clear the last worker STATE so readiness below provably comes from THIS pane.
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

  const workerWasmHeapBytes = async (): Promise<number> => {
    let heap = 0
    await expect
      .poll(
        async () => {
          heap = await orcaPage.evaluate(
            () =>
              (
                (window as unknown as WorkerRenderProbe).__atermWorkerRenderState as
                  | { wasmHeapBytes?: number }
                  | undefined
              )?.wasmHeapBytes ?? 0
          )
          return heap
        },
        { timeout: 10_000, message: 'worker state should report the wasm heap' }
      )
      .toBeGreaterThan(0)
    return heap
  }

  return { openWorkerPane, workerWasmHeapBytes }
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
    orcaPage
  }, testInfo) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)
    // Opt INTO the worker render path (the e2e suite defaults it off; production is on).
    // Force CPU worker engines: a GPU worker engine carries an undedupable per-pane wgpu
    // device + device-pixel swapchain (tens of MB) that would swamp the font-payload
    // signal this gate measures; CPU engines share the interned fonts in the wasm heap.
    await orcaPage.evaluate(() => {
      ;(window as unknown as WorkerRenderProbe).__atermWorkerRender = true
      ;(window as unknown as { __atermGpuDisabled?: boolean }).__atermGpuDisabled = true
    })

    const { openWorkerPane, workerWasmHeapBytes } = workerPaneProbes(orcaPage)

    // Panes 1-2 absorb the one-time costs (worker spawn, wasm modules, first-engine
    // atlas/parse warmup — pane 1's heap still settles by tens of MB while those
    // materialize); measure the pane2→pane3 delta so the gate isolates the steady
    // marginal per-pane cost.
    await openWorkerPane(1)
    await openWorkerPane(2)
    const afterSecond = await workerWasmHeapBytes()

    await openWorkerPane(3)
    const afterThird = await workerWasmHeapBytes()

    const marginalMB = (afterThird - afterSecond) / (1024 * 1024)
    const line =
      `[aterm-worker-memory] worker wasm heap after pane 2: ${(afterSecond / (1024 * 1024)).toFixed(1)} MB; ` +
      `after pane 3: ${(afterThird / (1024 * 1024)).toFixed(1)} MB → marginal ` +
      `${marginalMB.toFixed(1)} MB/pane (gate: < 48 MB/pane; ` +
      `a per-pane font payload would add hundreds of MB).`
    // eslint-disable-next-line no-console
    console.log(`\n${line}\n`)
    testInfo.annotations.push({ type: 'aterm-worker-memory', description: line })

    expect(
      marginalMB,
      'marginal per-pane cost must exclude the font payload (shared worker + interned fonts)'
    ).toBeLessThan(48)
  })

  // E1 LAZY-FONT GATE, classed: the ~183MB colour-emoji face must NOT ship until
  // an emoji actually renders. (The TEXT class is allowed to arrive early — any
  // non-ASCII glyph in the host's shell prompt is a legitimate miss, so an
  // absolute "ASCII heap" bound would be hostage to the machine's prompt; the
  // emoji face is the payload whale and its laziness is what this pins.)
  test('the emoji face ships only after an emoji actually renders (E1 lazy fonts)', async ({
    orcaPage
  }, testInfo) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)
    await orcaPage.evaluate(() => {
      ;(window as unknown as WorkerRenderProbe).__atermWorkerRender = true
      ;(window as unknown as { __atermGpuDisabled?: boolean }).__atermGpuDisabled = true
    })
    const { openWorkerPane, workerWasmHeapBytes } = workerPaneProbes(orcaPage)
    type DeliveryLog = { __atermFontClassDeliveries?: { class: string; bytes: number }[] }
    const deliveredClasses = (): Promise<string[]> =>
      orcaPage.evaluate(() =>
        ((window as unknown as DeliveryLog).__atermFontClassDeliveries ?? []).map((d) => d.class)
      )

    await openWorkerPane(1)
    await openWorkerPane(2)
    const beforeMB = (await workerWasmHeapBytes()) / (1024 * 1024)
    const before = await deliveredClasses()
    expect(
      before.filter((cls) => cls === 'emoji'),
      'no emoji rendered yet → the colour face must not have shipped'
    ).toEqual([])

    // Drive an emoji through the REAL UX seam: focus the pane and run a command
    // whose OUTPUT is 😀 (the typed line itself is pure ASCII — printf's \x
    // escapes emit the UTF-8 bytes shell-side, so no astral char rides the
    // keyboard input path). The engine renders it → miss → classed delivery.
    const ptyId = await waitForActivePanePtyId(orcaPage)
    await orcaPage.locator(`[data-pty-id="${ptyId}"] [data-testid="aterm-canvas"]`).first().click()
    await orcaPage.keyboard.type("printf '\\xf0\\x9f\\x98\\x80\\n'", { delay: 10 })
    await orcaPage.keyboard.press('Enter')

    await expect
      .poll(async () => (await deliveredClasses()).filter((cls) => cls === 'emoji'), {
        timeout: 30_000,
        message: 'rendering an emoji should pull the colour face in, exactly once'
      })
      .toEqual(['emoji'])

    // On macOS the delivered face is Apple Color Emoji (~183MB): the worker heap
    // must grow by roughly it — proof the bytes were genuinely absent before.
    if (process.platform === 'darwin') {
      let grownMB = 0
      await expect
        .poll(
          async () => {
            grownMB = (await workerWasmHeapBytes()) / (1024 * 1024) - beforeMB
            return grownMB
          },
          { timeout: 30_000, message: 'the emoji face should intern into the worker heap' }
        )
        .toBeGreaterThan(100)
      const line =
        `[aterm-lazy-fonts] pre-emoji heap ${beforeMB.toFixed(1)} MB (classes so far: ` +
        `${before.join('+') || 'none'}); rendering 😀 pulled the colour face lazily: ` +
        `+${grownMB.toFixed(1)} MB.`
      // eslint-disable-next-line no-console
      console.log(`\n${line}\n`)
      testInfo.annotations.push({ type: 'aterm-lazy-fonts', description: line })
    }
  })
})
