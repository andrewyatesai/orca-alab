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
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import {
  installTerminalPtyWriteSpy,
  readTerminalPtyWrites
} from './helpers/terminal-pty-write-spy'

// CRITICAL regression guard: the daemon emulator deliberately does NOT reply to
// terminal queries (see src/main/daemon/session.test.ts "emulator does not reply
// to terminal queries"); the RENDERER is the authoritative responder. With the
// aterm renderer default-on, the kept xterm Terminal stays UNOPENED (a shim for
// serialize). aterm is now the authoritative responder for an aterm pane:
//   - CPR (\e[6n) + DA1 (\e[c) + DA2/DSR/DECRQM + OSC-11 (\e]11;?\a): the aterm
//     ENGINE answers these — connectPanePty drains the engine's response buffer
//     after each process() and forwards it to the PTY, while the unopened xterm
//     shim's OWN auto-replies for those are SUPPRESSED at its parser so they don't
//     double-answer. (aterm's DA1 is VT420 + Sixel, so apps send inline images.)
//   - CSI 14t/16t (pixel size): the engine has no canvas/window callback, so the
//     renderer-side responder answers these from the live canvas pixel size.
// This proves all of them round-trip to the PTY exactly once: a child program
// issues each query and reads the reply back, and the main-process pty:write spy
// confirms the reply reached the PTY (and that CPR/DA1 appear exactly once).

function queryReplyScript(runId: string): string {
  // Issue each query in turn, read the reply with a short timeout, and print it
  // base64-encoded so the captured bytes survive the terminal buffer verbatim.
  return `
const queries = [
  ['CPR', '\\u001b[6n'],
  ['DA1', '\\u001b[c'],
  ['OSC11', '\\u001b]11;?\\u0007'],
  ['PX14', '\\u001b[14t'],
  ['PX16', '\\u001b[16t']
]
process.stdin.setEncoding('latin1')
if (process.stdin.isTTY) process.stdin.setRawMode(true)
process.stdin.resume()
process.stdout.write('QUERY_READY_${runId}\\n')
let i = 0
function next() {
  if (i >= queries.length) { process.exit(0); return }
  const [label, seq] = queries[i]
  let buf = ''
  const onData = (chunk) => {
    buf += chunk
  }
  process.stdin.on('data', onData)
  process.stdout.write(seq)
  setTimeout(() => {
    process.stdin.removeListener('data', onData)
    const encoded = Buffer.from(buf, 'latin1').toString('base64')
    process.stdout.write('REPLY_${runId}_' + label + ':' + encoded + '\\n')
    i += 1
    next()
  }, 1000)
}
next()
`
}

function decodeReply(content: string, runId: string, label: string): string | null {
  const re = new RegExp(`REPLY_${runId}_${label}:([A-Za-z0-9+/=]*)`)
  const match = re.exec(content)
  if (!match) {
    return null
  }
  return Buffer.from(match[1], 'base64').toString('latin1')
}

test.describe('aterm renderer query replies', () => {
  test('CPR / DA1 / OSC-11 queries round-trip to the PTY via aterm engine drain', async ({
    electronApp,
    orcaPage
  }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    // Explicit ON wins over the suite-wide opt-out (the default users hit).
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
    await installTerminalPtyWriteSpy(electronApp)

    const runId = randomUUID().slice(0, 8)
    const scriptPath = path.join(
      process.env.TMPDIR ?? '/tmp',
      `aterm-query-replies-${runId}.js`
    )
    writeFileSync(scriptPath, queryReplyScript(runId))

    try {
      await sendToTerminal(orcaPage, ptyId, `node ${JSON.stringify(scriptPath)}\r`)
      await waitForTerminalOutput(orcaPage, `QUERY_READY_${runId}`, 15_000)

      // Wait until the child has printed all reply lines (it exits after PX16).
      await waitForTerminalOutput(orcaPage, `REPLY_${runId}_PX16:`, 20_000, 40_000)

      const content = await getTerminalContent(orcaPage, 20_000)

      // ESC built at runtime (not a source control char) so the regexes below
      // stay free of the no-control-regex lint while still matching real bytes.
      const ESC = String.fromCharCode(27)
      const afterCsi = (reply: string | null): string =>
        reply && reply.startsWith(`${ESC}[`) ? reply.slice(2) : ''
      const afterOsc = (reply: string | null): string =>
        reply && reply.startsWith(`${ESC}]`) ? reply.slice(2) : ''

      // 1) The child captured a real CPR reply: ESC [ <row> ; <col> R.
      const cpr = decodeReply(content, runId, 'CPR')
      expect(cpr, 'captured a CPR reply').not.toBeNull()
      expect(afterCsi(cpr), 'CPR reply matches ESC[<row>;<col>R').toMatch(/^\d+;\d+R/)

      // 2) DA1: a device-attributes report ESC [ ? ... c.
      const da1 = decodeReply(content, runId, 'DA1')
      expect(da1, 'captured a DA1 reply').not.toBeNull()
      expect(afterCsi(da1), 'DA1 reply matches ESC[?...c').toMatch(/^\?[\d;]*c/)
      // aterm rasterizes Sixel (+ Kitty/iTerm2), so an aterm pane's DA1 advertises
      // the Sixel bit (param 4) — apps gate inline-image support on this.
      expect(afterCsi(da1), 'aterm DA1 advertises Sixel (param 4)').toMatch(/[?;]4[;c]/)

      // 3) OSC-11 background color: the UNOPENED xterm shim has no color service
      //    so it does NOT auto-reply to OSC 10/11 — the aterm renderer (which owns
      //    the theme) answers ESC ] 11 ; rgb:RRRR/GGGG/BBBB BEL from its seeded bg.
      const osc11 = decodeReply(content, runId, 'OSC11')
      expect(osc11, 'captured an OSC-11 reply').not.toBeNull()
      expect(afterOsc(osc11), 'OSC-11 reply names the 11 background color').toMatch(
        /^11;rgb:[0-9a-f]{4}\/[0-9a-f]{4}\/[0-9a-f]{4}/
      )

      // 4) CSI 14t (text-area pixel size): the aterm renderer is authoritative
      //    (unopened xterm + headless daemon can't know pixel size). Reply shape
      //    is ESC [ 4 ; heightPx ; widthPx t with real (>0) device px.
      const px14 = decodeReply(content, runId, 'PX14')
      expect(px14, 'captured a CSI 14t reply').not.toBeNull()
      expect(afterCsi(px14), 'CSI 14t reply matches ESC[4;H;Wt').toMatch(/^4;[1-9]\d*;[1-9]\d*t/)

      // 5) CSI 16t (cell pixel size): ESC [ 6 ; cellHpx ; cellWpx t.
      const px16 = decodeReply(content, runId, 'PX16')
      expect(px16, 'captured a CSI 16t reply').not.toBeNull()
      expect(afterCsi(px16), 'CSI 16t reply matches ESC[6;H;Wt').toMatch(/^6;[1-9]\d*;[1-9]\d*t/)

      // 6) Authoritative round-trip proof: the renderer-generated replies were
      //    sent to the PTY (pty:write). CPR/DA1/OSC-11 come from the aterm engine's
      //    drained response buffer, and 14t/16t from the renderer pixel responder —
      //    all reach the PTY, not just the xterm DOM path.
      const writes = (await readTerminalPtyWrites(electronApp)).join('')
      expect(writes, 'a CPR reply hit the PTY').toMatch(new RegExp(`${ESC}\\[\\d+;\\d+R`))
      expect(writes, 'a DA1 reply hit the PTY').toMatch(new RegExp(`${ESC}\\[\\?[\\d;]*c`))
      expect(writes, 'an OSC-11 reply hit the PTY').toMatch(new RegExp(`${ESC}\\]11;rgb:`))
      expect(writes, 'a CSI 14t reply hit the PTY').toMatch(
        new RegExp(`${ESC}\\[4;[1-9]\\d*;[1-9]\\d*t`)
      )
      expect(writes, 'a CSI 16t reply hit the PTY').toMatch(
        new RegExp(`${ESC}\\[6;[1-9]\\d*;[1-9]\\d*t`)
      )
      // 7) NO double-answer: aterm now drains its OWN CPR/DA1 and the xterm shim's
      //    auto-replies for those are suppressed at the parser. The child issued each
      //    query exactly once, so each reply must appear in the PTY writes exactly
      //    once — a count > 1 means both aterm AND xterm answered (the regression
      //    this migration's parser-suppression prevents).
      const countMatches = (re: RegExp): number => (writes.match(re) ?? []).length
      expect(
        countMatches(new RegExp(`${ESC}\\[\\d+;\\d+R`, 'g')),
        'exactly one CPR reply (no aterm+xterm double-answer)'
      ).toBe(1)
      expect(
        countMatches(new RegExp(`${ESC}\\[\\?[\\d;]*c`, 'g')),
        'exactly one DA1 reply (no aterm+xterm double-answer)'
      ).toBe(1)
    } finally {
      await sendToTerminal(orcaPage, ptyId, '\x03').catch(() => undefined)
      rmSync(scriptPath, { force: true })
    }
  })
})
