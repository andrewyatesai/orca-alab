import { randomUUID } from 'node:crypto'
import { rmSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { test, expect } from './helpers/orca-app'
import {
  getTerminalContent,
  sendToTerminal,
  waitForActivePanePtyId,
  waitForTerminalOutput
} from './helpers/terminal'
import { waitForActiveAtermController } from './helpers/aterm-controller'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import { installTerminalPtyWriteSpy, readTerminalPtyWrites } from './helpers/terminal-pty-write-spy'

// Restores the coverage the xterm removal flagged: the aterm ENGINE now owns DEC
// mode 2031 (color-scheme update notifications). This proves the mode-2031 path
// end-to-end so the "random characters on restart" regression stays covered:
//   1. Feeding `CSI ?2031h` through the engine flips its real getter
//      (is_color_scheme_updates_mode), surfaced via pane.atermController; `?2031l`
//      clears it. (The controller passthrough already exists — no new hook.)
//   2. A child issues a DEC color-scheme query (`CSI ?996n`) and reads the engine's
//      `CSI ?997;Ps n` reply back, and the main-process pty:write spy confirms the
//      reply actually reached the PTY (the aterm engine is the authoritative
//      responder — same drain → PTY path as aterm-query-replies.spec.ts).
//   3. With mode 2031 active across a hide/restore-style re-process cycle, NO stray
//      / random bytes (literal `?2031` / `?997` escape text) appear in the rendered
//      grid — the engine CONSUMES the sequences, it does not echo them as the
//      "random characters" the regression produced.

type Mode2031Probe = {
  process: (data: string) => void
  isColorSchemeUpdatesMode: () => boolean
  rowText: (row: number) => string | undefined
  gridSize: () => { cols: number; rows: number }
}

function findController(): Mode2031Probe {
  const managers = (window as unknown as { __paneManagers?: Map<string, unknown> }).__paneManagers
  for (const m of managers?.values() ?? []) {
    const mgr = m as {
      getActivePane?: () => { atermController?: Mode2031Probe | null } | null
      getPanes?: () => { atermController?: Mode2031Probe | null }[]
    }
    const pane = mgr.getActivePane?.() ?? mgr.getPanes?.()[0] ?? null
    if (pane?.atermController) {
      return pane.atermController
    }
  }
  throw new Error('no aterm controller')
}

// A child that issues a DEC color-scheme query and prints the reply base64-encoded
// (so the captured bytes survive the terminal buffer verbatim), mirroring
// aterm-query-replies.spec.ts.
function colorSchemeQueryScript(runId: string): string {
  return `
process.stdin.setEncoding('latin1')
if (process.stdin.isTTY) process.stdin.setRawMode(true)
process.stdin.resume()
process.stdout.write('CS_READY_${runId}\\n')
let buf = ''
const onData = (chunk) => { buf += chunk }
process.stdin.on('data', onData)
process.stdout.write('\\u001b[?996n')
setTimeout(() => {
  process.stdin.removeListener('data', onData)
  const encoded = Buffer.from(buf, 'latin1').toString('base64')
  process.stdout.write('CS_REPLY_${runId}:' + encoded + '\\n')
  process.exit(0)
}, 1000)
`
}

function decodeReply(content: string, runId: string): string | null {
  const re = new RegExp(`CS_REPLY_${runId}:([A-Za-z0-9+/=]*)`)
  const match = re.exec(content)
  return match ? Buffer.from(match[1], 'base64').toString('latin1') : null
}

test.describe('aterm color-scheme (DEC mode 2031)', () => {
  test('engine tracks mode 2031, answers CSI ?996n, and emits no stray bytes', async ({
    electronApp,
    orcaPage
  }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    await orcaPage.evaluate(() => {
      ;(window as unknown as { __atermRendererEnabled?: boolean }).__atermRendererEnabled = true
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

    // --- 1) ENGINE MODE GETTER -----------------------------------------------
    // Feed DECSET/DECRST ?2031 directly through the engine (the PTY-output path)
    // and assert the real getter flips. A broken engine mode handler (or a dropped
    // controller passthrough) makes these false/true respectively.
    const modeStates = await orcaPage.evaluate((findSrc: string) => {
      // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
      const find = new Function(`return (${findSrc})()`) as () => Mode2031Probe
      const ctrl = find()
      const before = ctrl.isColorSchemeUpdatesMode()
      ctrl.process('\x1b[?2031h')
      const afterSet = ctrl.isColorSchemeUpdatesMode()
      ctrl.process('\x1b[?2031l')
      const afterReset = ctrl.isColorSchemeUpdatesMode()
      // Re-enable for the stray-byte cycle below.
      ctrl.process('\x1b[?2031h')
      const reEnabled = ctrl.isColorSchemeUpdatesMode()
      return { before, afterSet, afterReset, reEnabled }
    }, findController.toString())

    expect(modeStates.before, 'mode 2031 defaults OFF').toBe(false)
    expect(modeStates.afterSet, 'CSI ?2031h enables the color-scheme update mode').toBe(true)
    expect(modeStates.afterReset, 'CSI ?2031l disables it').toBe(false)
    expect(modeStates.reEnabled, 'CSI ?2031h re-enables it').toBe(true)

    // --- 2) DSR ?996n ROUND-TRIP ---------------------------------------------
    await installTerminalPtyWriteSpy(electronApp)
    const runId = randomUUID().slice(0, 8)
    const scriptPath = path.join(process.env.TMPDIR ?? '/tmp', `aterm-color-scheme-${runId}.js`)
    writeFileSync(scriptPath, colorSchemeQueryScript(runId))

    try {
      await sendToTerminal(orcaPage, ptyId, `node ${JSON.stringify(scriptPath)}\r`)
      await waitForTerminalOutput(orcaPage, `CS_READY_${runId}`, 15_000)
      await waitForTerminalOutput(orcaPage, `CS_REPLY_${runId}:`, 20_000, 40_000)

      const content = await getTerminalContent(orcaPage, 20_000)
      const ESC = String.fromCharCode(27)
      const reply = decodeReply(content, runId)
      expect(reply, 'the child captured a color-scheme reply').not.toBeNull()
      // The aterm engine answers DSR ?996n with CSI ? 997 ; Ps n (Ps = 1 dark, 2 light).
      expect(reply, 'CSI ?996n yields ESC[?997;<1|2>n').toMatch(
        new RegExp(`^${ESC}\\[\\?997;[12]n$`)
      )

      // Authoritative round-trip proof: the reply reached the PTY (pty:write).
      const writes = (await readTerminalPtyWrites(electronApp)).join('')
      expect(writes, 'a CSI ?997 color-scheme reply hit the PTY').toMatch(
        new RegExp(`${ESC}\\[\\?997;[12]n`)
      )
    } finally {
      await sendToTerminal(orcaPage, ptyId, '\x03').catch(() => undefined)
      rmSync(scriptPath, { force: true })
    }

    // --- 3) NO STRAY BYTES ON A HIDE/RESTORE CYCLE ---------------------------
    // The "random characters on restart" regression surfaced as a TUI's replayed
    // mode-2031 traffic leaking into the rendered grid as literal escape fragments.
    // Simulate a restore-style burst with mode 2031 active: clear+home, a toggle of
    // the subscribe sequence (the exact private-mode bytes a cold-restore replays),
    // and a neutral visible marker — all in one process() call. The engine must
    // CONSUME every control sequence; the grid must hold ONLY the marker text, with
    // no leftover printable fragments (e.g. `?`, `h`, `n`, digits) from the
    // sequences and no ESC/control characters.
    const MARKER = 'VISIBLE_AFTER_RESTORE_ZQX'
    const grid = await orcaPage.evaluate(
      ({ findSrc, marker }) => {
        // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
        const find = new Function(`return (${findSrc})()`) as () => Mode2031Probe
        const ctrl = find()
        // Toggle subscribe off/on (a restore can carry both), embedded around the
        // marker so any byte the parser fails to consume would land NEXT to it.
        ctrl.process(`\x1b[2J\x1b[H\x1b[?2031h\x1b[?2031l\x1b[?2031h${marker}\r\n`)
        const { rows } = ctrl.gridSize()
        const text: string[] = []
        for (let r = 0; r < rows; r++) {
          const t = ctrl.rowText(r)
          if (t !== undefined) {
            text.push(t)
          }
        }
        return { text: text.join('\n'), modeStillOn: ctrl.isColorSchemeUpdatesMode() }
      },
      { findSrc: findController.toString(), marker: MARKER }
    )

    // The visible content rendered…
    expect(grid.text, 'the visible text after the control burst is rendered').toContain(MARKER)
    // …and mode 2031 is still tracked (the engine processed ?2031h, not echoed it).
    expect(grid.modeStillOn, 'mode 2031 is still active after the restore burst').toBe(true)
    // …and the grid, with the known marker and all whitespace removed, is EMPTY:
    // the control sequences left NO stray printable fragment behind. A broken parser
    // that echoed any of `\x1b[ ? 2 0 3 1 h l` would leave residue here.
    const residue = grid.text.replaceAll(MARKER, '').replace(/\s+/g, '')
    expect(
      residue,
      `only the marker should render (stray residue: ${JSON.stringify(residue)})`
    ).toBe('')
    // Defense in depth: no ESC byte leaked into the rendered text either.
    expect(grid.text.includes(String.fromCharCode(27)), 'no ESC byte in the grid').toBe(false)
  })
})
