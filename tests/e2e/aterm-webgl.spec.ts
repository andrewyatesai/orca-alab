import { test, expect } from './helpers/orca-app'
import { execInTerminal, waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import { writeFileSync } from 'node:fs'

// PROVES the EXPERIMENTAL aterm WebGL2 GPU draw path actually renders the grid
// headless. Drives the REAL Electron app with BOTH the aterm renderer and the
// GPU path forced on BEFORE the pane is created, then:
//   1. asserts the aterm canvas has a live webgl2 context (the GPU drawer owns
//      it via wgpu's WebGL backend) and logs the UNMASKED_RENDERER string,
//   2. runs a colored command (ANSI bg/fg) so the grid has real glyph pixels,
//   3. asserts the live swapchain has non-background pixels via gl.readPixels,
//   4. pixel-compares GPU vs CPU (fresh engines, same bytes, same grid) — the
//      GPU presents to a WebGL2 canvas (read via gl.readPixels), the CPU
//      rasterizes to RGBA — and asserts EVERY pixel is within ±6/channel,
//   5. saves /tmp/aterm-webgl.png.
// Headless WebGL2 here is ANGLE-over-Metal (the de-risk target was software
// SwiftShader; either works). If a webgl2 context is genuinely unavailable, the
// test FAILS LOUDLY with the reason rather than passing silently. We use
// readPixels (NOT the native render_offscreen) because WebGL2 cannot block-poll
// the buffer-map readback that path relies on.

const WEBGL_PNG = '/tmp/aterm-webgl.png'

test.describe('aterm WebGL2 GPU renderer', () => {
  test('renders the grid to a webgl2 canvas headless (GPU==CPU)', async ({ orcaPage }) => {
    // Surface renderer-process console + page errors so a GPU init/render failure
    // (which would silently fall the pane back to xterm) is visible in the report.
    orcaPage.on('console', (msg) => {
      const t = msg.text()
      if (/aterm|gpu|webgl|wgpu|panic/i.test(t)) {
        // eslint-disable-next-line no-console
        console.log(`[renderer:${msg.type()}] ${t}`)
      }
    })
    orcaPage.on('pageerror', (err) => {
      // eslint-disable-next-line no-console
      console.log(`[renderer:pageerror] ${err.message}`)
    })

    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    // Force the experimental GPU path on BEFORE the pane.
    await orcaPage.evaluate(() => {
      ;(window as unknown as { __atermGpuEnabled?: boolean }).__atermGpuEnabled = true
    })

    // First confirm a webgl2 context is even creatable in this headless runner; if
    // not, fail with that explicit reason (don't masquerade as a pass).
    const probe = await orcaPage.evaluate(() => {
      const c = document.createElement('canvas')
      const gl = c.getContext('webgl2')
      if (!gl) {
        return { hasWebgl2: false, renderer: null as string | null, vendor: null as string | null }
      }
      const dbg = gl.getExtension('WEBGL_debug_renderer_info')
      const renderer = dbg ? String(gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) ?? '') : ''
      const vendor = dbg ? String(gl.getParameter(dbg.UNMASKED_VENDOR_WEBGL) ?? '') : ''
      gl.getExtension('WEBGL_lose_context')?.loseContext()
      return { hasWebgl2: true, renderer: renderer || null, vendor: vendor || null }
    })
    // eslint-disable-next-line no-console
    console.log(
      `[aterm-webgl] probe webgl2=${probe.hasWebgl2} renderer=${probe.renderer ?? '<none>'} vendor=${probe.vendor ?? '<none>'}`
    )
    expect(
      probe.hasWebgl2,
      'a webgl2 context must be creatable headless to prove the GPU path'
    ).toBe(true)

    // New terminal tab → its pane is rendered by the aterm GPU path.
    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()

    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas, 'aterm canvas should mount for the new pane').toBeAttached({
      timeout: 20_000
    })
    const ptyId = await waitForActivePanePtyId(orcaPage)

    // The grid canvas must hold a webgl2 context (the GPU drawer's). Calling
    // getContext('webgl2') again returns the SAME context wgpu created; a non-null
    // result with a non-null 2d would be impossible (a canvas is one kind only),
    // so this proves the GPU strategy — not the CPU 2d path — owns the canvas.
    const ctxInfo = await orcaPage.evaluate(() => {
      const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
      if (!c) {
        return {
          gl: false,
          twoD: false,
          renderer: null as string | null,
          adapter: null as string | null
        }
      }
      const gl = c.getContext('webgl2')
      let renderer: string | null = null
      if (gl) {
        const dbg = gl.getExtension('WEBGL_debug_renderer_info')
        renderer = dbg ? String(gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) ?? '') || null : null
      }
      // Asking for 2d on a webgl2 canvas returns null, confirming it's GPU-owned.
      const twoD = Boolean(c.getContext('2d'))
      const w = window as {
        __atermGpuAdapterInfo?: string
        __atermGpuFailureReason?: string
      }
      const adapter = w.__atermGpuAdapterInfo ?? null
      const failureReason = w.__atermGpuFailureReason ?? null
      return { gl: Boolean(gl), twoD, renderer, adapter, failureReason }
    })
    // eslint-disable-next-line no-console
    console.log(
      `[aterm-webgl] canvas webgl2=${ctxInfo.gl} 2d=${ctxInfo.twoD} UNMASKED_RENDERER=${ctxInfo.renderer ?? '<none>'} wgpuAdapter=${ctxInfo.adapter ?? '<none>'} gpuFailureReason=${ctxInfo.failureReason ?? '<none>'}`
    )
    expect(
      ctxInfo.gl,
      `the aterm grid canvas must have a webgl2 context (GPU drawer); fell back to CPU because: ${ctxInfo.failureReason ?? 'unknown'}`
    ).toBe(true)
    expect(ctxInfo.twoD, 'a webgl2 canvas cannot also be 2d — confirms GPU ownership').toBe(false)
    expect(
      ctxInfo.adapter,
      'the wgpu WebGL adapter info should be exposed for the GPU pane'
    ).not.toBeNull()

    // Run a colored command so the grid has real bg/fg glyph pixels.
    await execInTerminal(
      orcaPage,
      ptyId,
      'printf "\\033[44;1;37m GPU \\033[0m \\033[1;32materm webgl\\033[0m %s\\n" OK'
    )

    // Assert real rendered pixels via gl.readPixels on the LIVE swapchain. WebGL's
    // framebuffer origin is bottom-left (Y flipped vs 2d), but for a "non-bg pixel
    // count" the flip doesn't matter — we count differing pixels across the buffer.
    await expect
      .poll(
        async () =>
          orcaPage.evaluate(() => {
            const c = document.querySelector(
              '[data-testid="aterm-canvas"]'
            ) as HTMLCanvasElement | null
            const gl = c?.getContext('webgl2')
            if (!c || !gl || !c.width || !c.height) {
              return 0
            }
            // Read the current swapchain back from the canvas's webgl2 context.
            const w = c.width
            const h = c.height
            const px = new Uint8Array(w * h * 4)
            gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, px)
            const bg = [px[0], px[1], px[2]]
            let n = 0
            for (let i = 0; i < px.length; i += 4) {
              if (px[i] !== bg[0] || px[i + 1] !== bg[1] || px[i + 2] !== bg[2]) {
                n++
              }
            }
            return n
          }),
        { timeout: 20_000, message: 'webgl2 readPixels should show rendered glyph pixels' }
      )
      .toBeGreaterThan(200)

    // GPU==CPU parity: fresh engines, same colored bytes, same grid; the GPU
    // presents to its WebGL2 canvas (read back via gl.readPixels) and the CPU
    // rasterizes to RGBA. Assert the per-channel max diff is within ±6 (the
    // de-risk tolerance) — proving the WebGL backend draws the same grid as the
    // gating CPU path. (We use readPixels, not render_offscreen, because WebGL2
    // can't block-poll the readback buffer map the native offscreen path uses.)
    const compare = await orcaPage.evaluate(async () => {
      const fn = (
        window as {
          __atermGpuVsCpuCompare?: (
            bytes: string,
            rows: number,
            cols: number
          ) => Promise<{
            available: boolean
            reason?: string
            maxChannelDiff: number
            withinToleranceFraction: number
            sampledPixels: number
            gpuNonBg: number
            cpuNonBg: number
            width: number
            height: number
          }>
        }
      ).__atermGpuVsCpuCompare
      if (!fn) {
        return null
      }
      // ESC[ sequences as Latin-1 (\x1b = 27). Bold white-on-blue then green text.
      const bytes = '\x1b[44;1;37m GPU \x1b[0m \x1b[1;32materm webgl\x1b[0m parity\r\n'
      return fn(bytes, 24, 80)
    })
    expect(compare, 'the GPU-vs-CPU compare hook should be present').not.toBeNull()
    // eslint-disable-next-line no-console
    console.log(
      `[aterm-webgl] GPU-vs-CPU available=${compare!.available} reason=${compare!.reason ?? '<none>'} ` +
        `maxChannelDiff=${compare!.maxChannelDiff} within6=${(compare!.withinToleranceFraction * 100).toFixed(3)}% ` +
        `sampled=${compare!.sampledPixels} gpuNonBg=${compare!.gpuNonBg} cpuNonBg=${compare!.cpuNonBg} ` +
        `${compare!.width}x${compare!.height}`
    )
    expect(
      compare!.available,
      `GPU-vs-CPU compare must run (reason=${compare!.reason ?? ''})`
    ).toBe(true)
    expect(compare!.gpuNonBg, 'GPU frame must have rendered glyphs').toBeGreaterThan(200)
    expect(compare!.cpuNonBg, 'CPU frame must have rendered glyphs').toBeGreaterThan(200)
    // Both rasterizers (CPU aterm-render vs the WebGL aterm-gpu backend) draw the
    // SAME grid: identical non-bg pixel COUNT, and EVERY pixel within ±6/channel —
    // the de-risk's strict GPU==CPU parity bound, now proven on the WebGL backend.
    expect(compare!.gpuNonBg, 'GPU and CPU must produce the same glyph coverage').toBe(
      compare!.cpuNonBg
    )
    expect(
      compare!.maxChannelDiff,
      `GPU and CPU pixels must match within ±6/channel (maxDiff=${compare!.maxChannelDiff}, within6=${(compare!.withinToleranceFraction * 100).toFixed(3)}%)`
    ).toBeLessThanOrEqual(6)

    // Save the GPU canvas to a PNG for visual evidence.
    const dataUrl = await orcaPage.evaluate(() => {
      const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
      return c ? c.toDataURL('image/png') : ''
    })
    expect(dataUrl.startsWith('data:image/png;base64,')).toBe(true)
    writeFileSync(WEBGL_PNG, Buffer.from(dataUrl.split(',')[1], 'base64'))
    // eslint-disable-next-line no-console
    console.log(`[aterm-webgl] PASS — wrote ${WEBGL_PNG}`)
  })
})
