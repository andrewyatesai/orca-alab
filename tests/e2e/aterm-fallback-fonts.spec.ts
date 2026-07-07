import { test, expect } from './helpers/orca-app'
import { getTerminalLogicalText, waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveAtermController } from './helpers/aterm-controller'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import { atermCanvasReady } from './helpers/aterm-canvas-pixels'
import { existsSync } from 'node:fs'
import { writeFileSync } from 'node:fs'

// RENDER proof of the host → IPC → set_fallback_font / add_fallback_font chain:
// scripts the primary JetBrains Mono face LACKS (CJK, Arabic, Devanagari) must
// render REAL glyphs end-to-end, not .notdef tofu. The main process reads the OS
// fallback faces and hands the bytes to the engine; the engine appends them as a
// fallback chain. This drives the REAL Electron app on the CPU path (clean headless
// pixel read; the fallback faces are shared with the GPU path) and, for each script:
//   (a) asserts the rendered row has REAL ink (non-bg pixel count well above a blank
//       control row) — it is NOT a blank cell, and
//   (b) asserts the script's row pixels DIFFER materially from the OTHER scripts'
//       rows — tofu would paint the SAME .notdef box for every missing glyph, so a
//       distinct fingerprint per script proves the chain resolved DIFFERENT real
//       faces, not one shared box.
// Scripts whose host font is reliably absent are skip-gated (clear message), never
// asserted falsely.

type Probe = { process: (d: string) => void; cellSizeCss: () => { width: number; height: number } }

// Resolve the controller BY PTY ID — the same identity the test drives bytes
// through. Manager-iteration order surfaces the bootstrap "Terminal 1" first, so
// a positional lookup injects into a DIFFERENT (hidden, GPU) pane than the one
// whose canvas is scanned.
function findController(ptyId: string): Probe {
  const managers = (window as unknown as { __paneManagers?: Map<string, unknown> }).__paneManagers
  for (const m of managers?.values() ?? []) {
    const mgr = m as {
      getPanes?: () => {
        atermController?: Probe | null
        container?: { dataset?: { ptyId?: string } }
      }[]
    }
    for (const pane of mgr.getPanes?.() ?? []) {
      if (pane?.container?.dataset?.ptyId === ptyId && pane.atermController) {
        return pane.atermController
      }
    }
  }
  throw new Error(`no aterm controller for pty ${ptyId}`)
}

// Per-script sample text + whether the host has a covering font. macOS ships CJK
// (Hiragino/PingFang), Arabic (GeezaPro/SFArabic/Arial Unicode), and Devanagari
// (DevanagariMT/Kohinoor); on other OSes we skip-gate any script with no candidate.
type ScriptCase = { name: string; text: string; rowOffset: number }

const SCRIPTS: ScriptCase[] = [
  // Each on its OWN row so a row-band scan isolates that script's glyphs.
  { name: 'CJK', text: '中文字符示例', rowOffset: 0 },
  { name: 'Arabic', text: 'العربية', rowOffset: 2 },
  { name: 'Devanagari', text: 'नमस्ते', rowOffset: 4 }
]

// Host font presence per script (macOS/Linux/Windows candidate paths mirroring
// src/main/terminal-fallback-fonts.ts). If NONE exist we skip-gate that script.
const HOST_FONT_CANDIDATES: Record<string, string[]> = {
  CJK: [
    '/System/Library/Fonts/PingFang.ttc',
    '/System/Library/Fonts/Hiragino Sans GB.ttc',
    '/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc',
    '/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc',
    'C:/Windows/Fonts/msyh.ttc',
    'C:/Windows/Fonts/simsun.ttc'
  ],
  Arabic: [
    '/System/Library/Fonts/Supplemental/GeezaPro.ttc',
    '/Library/Fonts/Arial Unicode.ttf',
    '/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf',
    'C:/Windows/Fonts/segoeui.ttf'
  ],
  Devanagari: [
    '/System/Library/Fonts/Supplemental/DevanagariMT.ttc',
    '/System/Library/Fonts/Kohinoor.ttc',
    '/Library/Fonts/Arial Unicode.ttf',
    '/usr/share/fonts/truetype/noto/NotoSansDevanagari-Regular.ttf',
    'C:/Windows/Fonts/Nirmala.ttf'
  ]
}

function hostHasFont(script: string): boolean {
  return (HOST_FONT_CANDIDATES[script] ?? []).some((p) => existsSync(p))
}

test.describe('aterm non-Latin fallback fonts', () => {
  test('CJK / Arabic / Devanagari render real glyphs (not tofu) via the fallback chain', async ({
    orcaPage
  }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    await orcaPage.evaluate(() => {
      // CPU path for a deterministic headless getImageData read; the fallback faces
      // are the same on the GPU path (parity proven in aterm-webgl.spec.ts).
      ;(window as unknown as { __atermGpuDisabled?: boolean }).__atermGpuDisabled = true
    })

    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()

    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas, 'aterm canvas should mount').toBeAttached({ timeout: 20_000 })
    const ptyId = await waitForActivePanePtyId(orcaPage)
    await waitForActiveAtermController(orcaPage)
    await expect
      .poll(async () => atermCanvasReady(orcaPage), {
        timeout: 20_000,
        message: 'aterm canvas should be ready to read'
      })
      .toBe(true)

    // Let the shell emit its startup prompt and go idle BEFORE injecting: the
    // controller shares the buffer with the live PTY, so a prompt arriving after
    // our clear+glyphs overwrites them (the pane reads blank). Wait for a settled
    // prompt line (ends with a shell sigil).
    await expect
      .poll(async () => /[%$#>]\s*$/.test((await getTerminalLogicalText(orcaPage)).trimEnd()), {
        timeout: 15_000,
        message: 'shell prompt should settle before injecting glyphs'
      })
      .toBe(true)

    // Decide which scripts to assert vs skip-gate up front (Node-side fs).
    const gated = SCRIPTS.map((s) => ({ ...s, present: hostHasFont(s.name) }))
    for (const s of gated) {
      if (!s.present) {
        // eslint-disable-next-line no-console
        console.log(`[aterm-fallback-fonts] SKIP ${s.name}: no host font on this OS`)
      }
    }
    const active = gated.filter((s) => s.present)
    test.skip(active.length === 0, 'no non-Latin host fonts available on this OS')

    // Build payload: clear+home, then each ACTIVE script on its own row. A trailing
    // BLANK row (control) sits below for the ink baseline.
    const controlRow = 8
    const lines = ['\x1b[2J\x1b[H']
    for (const s of active) {
      lines.push(`\x1b[${s.rowOffset + 1};1H`) // CUP: 1-based row, col 1
      lines.push(s.text)
    }
    const payload = lines.join('')

    const fingerprints = await orcaPage.evaluate(
      async ({ findSrc, payload, scripts, controlRow, ptyId }) => {
        // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
        const find = new Function(`return (${findSrc})`)() as (id: string) => Probe
        const ctrl = find(ptyId)
        ctrl.process(payload)
        // Wait two frames so the rAF-coalesced draw flushes all rows (the CJK +
        // Arabic + Devanagari runs are all painted in the next frame).
        await new Promise((res) => requestAnimationFrame(() => requestAnimationFrame(res)))

        // Scope to the pane the payload was driven into (by ptyId): a bare
        // querySelector returns the FIRST aterm canvas, which on GPU-capable hosts is a
        // different, webgl2-owned pane whose getContext('2d') is null. The forced-CPU
        // pane this test created is the one bound to ptyId.
        const managers = (
          window as unknown as {
            __paneManagers?: Map<
              string,
              {
                getPanes?: () => {
                  container?: {
                    dataset?: { ptyId?: string }
                    querySelector: (s: string) => Element | null
                  }
                }[]
              }
            >
          }
        ).__paneManagers
        let c: HTMLCanvasElement | null = null
        for (const mgr of managers?.values() ?? []) {
          for (const pane of mgr.getPanes?.() ?? []) {
            if (pane?.container?.dataset?.ptyId === ptyId) {
              c = pane.container.querySelector(
                '[data-testid="aterm-canvas"]'
              ) as HTMLCanvasElement | null
            }
          }
        }
        if (!c) {
          return null
        }
        const ctx = c.getContext('2d')
        if (!ctx) {
          return null
        }
        const dpr = window.devicePixelRatio || 1
        const cell = ctrl.cellSizeCss()
        const cellHpx = Math.max(1, Math.round(cell.height * dpr))
        const cellWpx = Math.max(1, Math.round(cell.width * dpr))
        const W = c.width
        const d = ctx.getImageData(0, 0, W, c.height).data
        const bg = [d[0], d[1], d[2]]
        const isBg = (i: number): boolean =>
          d[i] === bg[0] && d[i + 1] === bg[1] && d[i + 2] === bg[2]

        // Scan one cell-height row band over the left ~16 cells, returning the
        // non-bg ink count AND a coarse spatial fingerprint (which 8x4 sub-cells of
        // the band contain ink) so two scripts with similar ink counts but different
        // SHAPES still differ. Tofu boxes share the same fingerprint everywhere.
        const scanRow = (rowOffset: number): { ink: number; fp: string } => {
          const y0 = rowOffset * cellHpx
          const y1 = Math.min(c.height, y0 + cellHpx)
          const x1 = Math.min(W, cellWpx * 16)
          let ink = 0
          const GX = 16
          const GY = 4
          const grid = new Uint8Array(GX * GY)
          for (let y = y0; y < y1; y++) {
            for (let x = 0; x < x1; x++) {
              const i = (y * W + x) * 4
              if (!isBg(i)) {
                ink++
                const gx = Math.min(GX - 1, Math.floor((x / x1) * GX))
                const gy = Math.min(GY - 1, Math.floor(((y - y0) / (y1 - y0)) * GY))
                grid[gy * GX + gx] = 1
              }
            }
          }
          return { ink, fp: Array.from(grid).join('') }
        }

        const control = scanRow(controlRow)
        const out: Record<string, { ink: number; fp: string }> = { __control: control }
        for (const s of scripts) {
          out[s.name] = scanRow(s.rowOffset)
        }
        return out
      },
      {
        findSrc: findController.toString(),
        payload,
        scripts: active.map((s) => ({ name: s.name, rowOffset: s.rowOffset })),
        controlRow,
        ptyId
      }
    )

    expect(fingerprints, 'should scan the rendered rows').not.toBeNull()
    const control = fingerprints!.__control
    // The control (blank) row should be ~empty; allow a tiny cursor/edge artifact.
    expect(control.ink, 'the control row is blank (baseline)').toBeLessThan(20)

    // (a) REAL INK: every active script paints far more ink than the blank control.
    for (const s of active) {
      const f = fingerprints![s.name]
      expect(
        f.ink,
        `${s.name} renders real ink (>> the blank control's ${control.ink})`
      ).toBeGreaterThan(150)
    }

    // (b) NOT TOFU: distinct scripts paint DISTINCT shapes. If the fallback chain
    // failed and every missing glyph fell to the same .notdef box, the per-row
    // spatial fingerprints would be (near-)identical. Assert each pair of active
    // scripts has a materially different fingerprint.
    const fpDistance = (a: string, b: string): number => {
      let diff = 0
      for (let i = 0; i < Math.min(a.length, b.length); i++) {
        if (a[i] !== b[i]) {
          diff++
        }
      }
      return diff
    }
    for (let i = 0; i < active.length; i++) {
      for (let j = i + 1; j < active.length; j++) {
        const a = active[i]
        const b = active[j]
        const dist = fpDistance(fingerprints![a.name].fp, fingerprints![b.name].fp)
        expect(
          dist,
          `${a.name} and ${b.name} render DISTINCT glyph shapes (not the same .notdef tofu box)`
        ).toBeGreaterThan(4)
      }
    }

    // eslint-disable-next-line no-console
    console.log(
      `[aterm-fallback-fonts] PASS — real glyphs for: ${active.map((s) => s.name).join(', ')}`
    )

    const dataUrl = await orcaPage.evaluate(() => {
      const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
      return c ? c.toDataURL('image/png') : ''
    })
    if (dataUrl.startsWith('data:image/png;base64,')) {
      writeFileSync('/tmp/aterm-fallback-fonts.png', Buffer.from(dataUrl.split(',')[1], 'base64'))
    }
  })
})
