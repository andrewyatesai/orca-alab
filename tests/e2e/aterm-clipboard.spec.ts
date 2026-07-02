import { randomUUID } from 'node:crypto'
import { rmSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import type { ElectronApplication, Page } from '@stablyai/playwright-test'
import { test, expect } from './helpers/orca-app'
import { waitForActiveAtermController } from './helpers/aterm-controller'
import { sendToTerminal, waitForActivePanePtyId, waitForTerminalOutput } from './helpers/terminal'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import {
  clearTerminalPtyWriteLog,
  installTerminalPtyWriteSpy,
  readTerminalPtyWrites
} from './helpers/terminal-pty-write-spy'

// Proves the last two audited terminal capabilities WORK in the aterm-rendered
// pane (the default renderer), end to end:
//
//   1. BRACKETED PASTE — orc's paste path (helper-textarea InputEvent
//      insertFromPaste → controller pasteSink → pane.terminal.paste()) respects
//      the APP'S DECSET 2004 mode. The kept (unopened) xterm Terminal is fed all
//      PTY output, so its `modes.bracketedPasteMode` tracks the running shell;
//      xterm.paste() wraps with ESC[200~ … ESC[201~ when on, raw when off. The
//      wrapped/raw bytes are asserted via the main-process pty:write spy.
//   2. OSC-52 — a shell-emitted ESC]52;c;<base64> BEL reaches the shared OSC 52
//      parser handler (registered in onPaneCreated) and writes the decoded text
//      to the host clipboard via window.api.ui.writeClipboardText →
//      ipcRenderer.invoke('clipboard:writeText'), gated on the
//      terminalAllowOsc52Clipboard user setting. Captured via a main-process
//      spy on the clipboard:writeText IPC (the contextBridge-exposed
//      window.api.ui.writeClipboardText cannot be overridden from the page under
//      contextIsolation, so we mirror the proven pty:write spy and wrap the
//      main-process handler instead).

// ESC built at runtime (not a source control char) so the markers we assert on
// stay free of the no-control lint while still matching real bytes.
const ESC = String.fromCharCode(27)

/** Read the (unopened) xterm Terminal's bracketedPasteMode via the pane manager —
 *  the SAME object pane.terminal.paste() consults when wrapping a paste. */
async function readShimBracketedPasteMode(orcaPage: Page): Promise<boolean | null> {
  return orcaPage.evaluate(() => {
    const managers = (window as unknown as { __paneManagers?: Map<string, unknown> }).__paneManagers
    if (!managers) {
      return null
    }
    for (const manager of managers.values()) {
      const m = manager as {
        getActivePane?: () => { terminal?: { modes?: { bracketedPasteMode?: boolean } } } | null
        getPanes?: () => { terminal?: { modes?: { bracketedPasteMode?: boolean } } }[]
      }
      const pane = m.getActivePane?.() ?? m.getPanes?.()[0] ?? null
      if (pane?.terminal?.modes) {
        return pane.terminal.modes.bracketedPasteMode === true
      }
    }
    return null
  })
}

/** Wrap the main-process `clipboard:writeText` IPC handler so the e2e can read
 *  back every text the renderer (OSC 52 → window.api.ui.writeClipboardText)
 *  asked the host to copy. Mirrors terminal-pty-write-spy: Playwright can't
 *  observe ipcRenderer.invoke payloads, so we wrap main's invoke handler. */
async function installClipboardWriteSpy(app: ElectronApplication): Promise<void> {
  await app.evaluate(({ ipcMain }) => {
    const global = globalThis as unknown as {
      __clipboardWriteLog?: string[]
      __clipboardWriteSpyInstalled?: boolean
    }
    if (global.__clipboardWriteSpyInstalled) {
      return
    }
    global.__clipboardWriteLog = []
    const invokeHandlers = (
      ipcMain as unknown as {
        _invokeHandlers?: Map<string, (event: unknown, text: string) => unknown>
      }
    )._invokeHandlers
    const writeTextHandler = invokeHandlers?.get('clipboard:writeText')
    if (!writeTextHandler) {
      return
    }
    global.__clipboardWriteSpyInstalled = true
    invokeHandlers?.set('clipboard:writeText', async (event, text) => {
      global.__clipboardWriteLog!.push(text)
      return writeTextHandler(event, text)
    })
  })
}

async function readClipboardWrites(app: ElectronApplication): Promise<string[]> {
  return app.evaluate(() => {
    const global = globalThis as unknown as { __clipboardWriteLog?: string[] }
    return [...(global.__clipboardWriteLog ?? [])]
  })
}

/** Open a fresh aterm-rendered terminal pane and return its PTY id. */
async function openAtermTerminal(orcaPage: Page): Promise<string> {
  await waitForSessionReady(orcaPage)
  await waitForActiveWorktree(orcaPage)

  await orcaPage.getByRole('button', { name: 'New tab' }).click()
  await orcaPage
    .getByRole('menuitem', { name: /New Terminal/i })
    .first()
    .click()

  const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
  await expect(canvas, 'aterm canvas should mount').toBeAttached({ timeout: 20_000 })
  return waitForActivePanePtyId(orcaPage)
}

/** Dispatch orc's real paste path: an InputEvent('input', insertFromPaste) on the
 *  active aterm pane's .xterm-helper-textarea — the same event a programmatic /
 *  OS paste produces. The aterm textarea-input handler routes insertFromPaste to
 *  the controller's pasteSink → pane.terminal.paste(). */
async function dispatchAtermPaste(orcaPage: Page, text: string): Promise<void> {
  const ok = await orcaPage.evaluate((pasteText) => {
    const canvasEl = document.querySelector('[data-testid="aterm-canvas"]')
    const ta = canvasEl
      ?.closest('.xterm')
      ?.querySelector('.xterm-helper-textarea') as HTMLTextAreaElement | null
    if (!ta) {
      return false
    }
    ta.value = pasteText
    ta.dispatchEvent(
      new InputEvent('input', { data: pasteText, inputType: 'insertFromPaste', bubbles: true })
    )
    return true
  }, text)
  expect(ok, 'aterm pane should expose a helper textarea to paste into').toBe(true)
}

/** Write a raw control sequence straight onto the shim's output path
 *  (terminal.write) — the exact path PTY output uses to update terminal modes. */
async function writeToShim(orcaPage: Page, sequence: string): Promise<void> {
  const ok = await orcaPage.evaluate((seq) => {
    const managers = (window as unknown as { __paneManagers?: Map<string, unknown> }).__paneManagers
    if (!managers) {
      return false
    }
    for (const manager of managers.values()) {
      const m = manager as {
        getActivePane?: () => { terminal?: { write?: (d: string) => void } } | null
        getPanes?: () => { terminal?: { write?: (d: string) => void } }[]
      }
      const pane = m.getActivePane?.() ?? m.getPanes?.()[0] ?? null
      if (pane?.terminal?.write) {
        pane.terminal.write(seq)
        return true
      }
    }
    return false
  }, sequence)
  expect(ok, 'active pane terminal should accept a shim write').toBe(true)
}

test.describe('aterm renderer clipboard capabilities', () => {
  test('bracketed paste round-trips under aterm (wrapped when ON, raw when OFF)', async ({
    electronApp,
    orcaPage
  }) => {
    const ptyId = await openAtermTerminal(orcaPage)
    // The textarea 'input' paste listener is attached only when the async aterm
    // controller finishes loading — wait for it so the dispatched paste below
    // isn't silently dropped under parallel load (before the handler exists).
    await waitForActiveAtermController(orcaPage)
    await installTerminalPtyWriteSpy(electronApp)

    // 1) BRACKETED PASTE ON. Have the SHELL emit DECSET 2004h so BOTH the kept
    //    xterm shim and the aterm engine see the same mode the real app sees.
    await sendToTerminal(orcaPage, ptyId, `printf '\\033[?2004h'\r`)
    // Output is async, so poll the shim's tracked mode before pasting.
    await expect
      .poll(async () => readShimBracketedPasteMode(orcaPage), {
        timeout: 15_000,
        message: 'shim bracketedPasteMode did not turn on after DECSET 2004h'
      })
      .toBe(true)

    await clearTerminalPtyWriteLog(electronApp)
    await dispatchAtermPaste(orcaPage, 'PASTED')

    // The bytes sent to the PTY must be WRAPPED: ESC[200~ PASTED ESC[201~.
    await expect
      .poll(async () => (await readTerminalPtyWrites(electronApp)).join(''), {
        timeout: 15_000,
        message: 'bracketed paste markers did not reach the PTY'
      })
      .toContain(`${ESC}[200~PASTED${ESC}[201~`)

    // 2) BRACKETED PASTE OFF. Drive DECSET 2004l through the SHIM's output path
    //    directly — terminal.write() is the exact path PTY output takes to update
    //    modes.bracketedPasteMode. We do NOT use the interactive shell here: zsh's
    //    line editor re-emits ESC[?2004h on every prompt redraw, so a shell-driven
    //    reset is immediately undone by the next prompt. Writing the reset to the
    //    shim (with no shell Enter, so no prompt redraw) keeps the mode
    //    deterministically OFF for the paste that follows.
    await writeToShim(orcaPage, `${ESC}[?2004l`)
    await expect
      .poll(async () => readShimBracketedPasteMode(orcaPage), {
        timeout: 15_000,
        message: 'shim bracketedPasteMode did not turn off after DECSET 2004l'
      })
      .toBe(false)

    await clearTerminalPtyWriteLog(electronApp)
    await dispatchAtermPaste(orcaPage, 'RAWPASTE')

    await expect
      .poll(async () => (await readTerminalPtyWrites(electronApp)).join(''), {
        timeout: 15_000,
        message: 'raw paste text did not reach the PTY'
      })
      .toContain('RAWPASTE')

    const offWrites = (await readTerminalPtyWrites(electronApp)).join('')
    expect(offWrites, 'paste with bracketed mode OFF must NOT wrap with markers').not.toContain(
      `${ESC}[200~`
    )
    expect(offWrites, 'paste with bracketed mode OFF must NOT wrap with markers').not.toContain(
      `${ESC}[201~`
    )
  })

  test('OSC-52 clipboard copy writes the decoded text to the host clipboard', async ({
    electronApp,
    orcaPage
  }) => {
    await installClipboardWriteSpy(electronApp)

    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)
    // Enable the OSC-52 opt-in BEFORE the aterm pane is created so the gate the
    // shared handler reads (settingsRef.current.terminalAllowOsc52Clipboard) is
    // already true when the sequence arrives.
    await orcaPage.evaluate(async () => {
      await window.__store?.getState().updateSettings({ terminalAllowOsc52Clipboard: true })
    })

    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()
    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas, 'aterm canvas should mount').toBeAttached({ timeout: 20_000 })
    const ptyId = await waitForActivePanePtyId(orcaPage)

    const runId = randomUUID().slice(0, 8)
    const expected = `aterm-osc52-proof-${runId}`
    const scriptPath = path.join(process.env.TMPDIR ?? '/tmp', `aterm-osc52-${runId}.js`)
    // Emit the OSC-52 set-clipboard sequence from a child program so it travels
    // the real PTY→shim-parser path. base64 the payload in-process so we don't
    // depend on a `base64` binary or shell quoting.
    writeFileSync(
      scriptPath,
      `const text = ${JSON.stringify(expected)}\n` +
        `const b64 = Buffer.from(text, 'utf8').toString('base64')\n` +
        `process.stdout.write('\\u001b]52;c;' + b64 + '\\u0007')\n` +
        `process.stdout.write('OSC52_DONE_${runId}\\n')\n`
    )

    try {
      await sendToTerminal(orcaPage, ptyId, `node ${JSON.stringify(scriptPath)}\r`)
      await waitForTerminalOutput(orcaPage, `OSC52_DONE_${runId}`, 20_000, 12_000)

      // The shared OSC 52 handler must have decoded the base64 payload and asked
      // the host to copy the exact plaintext.
      await expect
        .poll(async () => readClipboardWrites(electronApp), {
          timeout: 15_000,
          message: 'OSC-52 did not write the decoded text to the host clipboard'
        })
        .toContain(expected)
    } finally {
      await sendToTerminal(orcaPage, ptyId, '\x03').catch(() => undefined)
      rmSync(scriptPath, { force: true })
    }
  })
})
