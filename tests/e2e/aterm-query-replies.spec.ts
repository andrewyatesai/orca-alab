import { randomUUID } from 'node:crypto'
import { existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { test, expect } from './helpers/orca-app'
import { sendToTerminal, waitForActivePanePtyId, waitForTerminalOutput } from './helpers/terminal'
import { waitForAtermControllerByPtyId } from './helpers/aterm-controller'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import { installTerminalPtyWriteSpy, readTerminalPtyWrites } from './helpers/terminal-pty-write-spy'

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

function queryReplyScript(runId: string, replyPath: string): string {
  // A child that issues each capability query, reads the reply back off its PTY
  // stdin, and writes a { label: base64(bytes) } map to `replyPath`. A single
  // PERSISTENT stdin listener attributes bytes to the outstanding query; the
  // aterm worker render path answers within a few ms, but reading the replies
  // back out of the RENDERED grid is racy (worker render lag + the post-exit
  // shell prompt repaint clears the printed lines), so the capture is written to
  // a file the test reads directly instead of scraped from terminal output.
  return `
const fs = require('node:fs')
const queries = [
  ['CPR', '\\u001b[6n'],
  ['DA1', '\\u001b[c'],
  ['OSC11', '\\u001b]11;?\\u0007'],
  ['PX14', '\\u001b[14t'],
  ['PX16', '\\u001b[16t']
]
const captured = {}
let current = null
process.stdin.setEncoding('latin1')
if (process.stdin.isTTY) process.stdin.setRawMode(true)
process.stdin.resume()
process.stdin.on('data', (c) => { if (current !== null) captured[current] = (captured[current] || '') + c })
process.stdout.write('QUERY_READY_${runId}\\n')
let i = 0
function next() {
  if (i >= queries.length) {
    const enc = {}
    for (const k of Object.keys(captured)) enc[k] = Buffer.from(captured[k], 'latin1').toString('base64')
    fs.writeFileSync(${JSON.stringify(replyPath)}, JSON.stringify(enc))
    process.stdout.write('QUERY_REPLIES_DONE_${runId}\\n')
    process.exit(0)
    return
  }
  const [label, seq] = queries[i]
  current = label
  process.stdout.write(seq)
  // The reply lands within a few ms; 400ms is a generous per-query window.
  setTimeout(() => { current = null; i += 1; next() }, 400)
}
next()
`
}

function decodeCapturedReply(replies: Record<string, string>, label: string): string | null {
  const b64 = replies[label]
  return typeof b64 === 'string' ? Buffer.from(b64, 'base64').toString('latin1') : null
}

/** Poll for the child's reply-capture file (written atomically after the last
 *  query) and return the parsed { label: base64 } map. */
async function readCapturedReplies(replyPath: string): Promise<Record<string, string>> {
  let replies: Record<string, string> = {}
  await expect
    .poll(
      () => {
        if (!existsSync(replyPath)) {
          return false
        }
        try {
          replies = JSON.parse(readFileSync(replyPath, 'utf8')) as Record<string, string>
          return true
        } catch {
          return false
        }
      },
      { timeout: 40_000, message: 'child never wrote the reply-capture file' }
    )
    .toBe(true)
  return replies
}

test.describe('aterm renderer query replies', () => {
  test('CPR / DA1 / OSC-11 queries round-trip to the PTY via aterm engine drain', async ({
    electronApp,
    orcaPage
  }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()

    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas, 'aterm canvas should mount').toBeAttached({ timeout: 20_000 })
    const ptyId = await waitForActivePanePtyId(orcaPage)
    // Wait for THIS pane's aterm controller (by ptyId): it is the authoritative query
    // responder, and on the worker render path the engine build can outlast the
    // child's per-query reply windows if the queries start before it attaches.
    await waitForAtermControllerByPtyId(orcaPage, ptyId)
    await installTerminalPtyWriteSpy(electronApp)

    const runId = randomUUID().slice(0, 8)
    const scriptPath = path.join(process.env.TMPDIR ?? '/tmp', `aterm-query-replies-${runId}.js`)
    const replyPath = `${scriptPath}.replies.json`
    writeFileSync(scriptPath, queryReplyScript(runId, replyPath))

    try {
      await sendToTerminal(orcaPage, ptyId, `node ${JSON.stringify(scriptPath)}\r`)
      await waitForTerminalOutput(orcaPage, `QUERY_READY_${runId}`, 15_000)

      // The child writes its captured replies to `replyPath` then exits; poll for
      // that FILE rather than scraping the rendered grid (which the worker render
      // path + shell repaint make racy). The file is the deterministic source.
      const replies = await readCapturedReplies(replyPath)

      // ESC built at runtime (not a source control char) so the regexes below
      // stay free of the no-control-regex lint while still matching real bytes.
      const ESC = String.fromCharCode(27)
      const afterCsi = (reply: string | null): string =>
        reply && reply.startsWith(`${ESC}[`) ? reply.slice(2) : ''
      const afterOsc = (reply: string | null): string =>
        reply && reply.startsWith(`${ESC}]`) ? reply.slice(2) : ''

      // 1) The child captured a real CPR reply: ESC [ <row> ; <col> R.
      const cpr = decodeCapturedReply(replies, 'CPR')
      expect(cpr, 'captured a CPR reply').not.toBeNull()
      expect(afterCsi(cpr), 'CPR reply matches ESC[<row>;<col>R').toMatch(/^\d+;\d+R/)

      // 2) DA1: a device-attributes report ESC [ ? ... c.
      const da1 = decodeCapturedReply(replies, 'DA1')
      expect(da1, 'captured a DA1 reply').not.toBeNull()
      expect(afterCsi(da1), 'DA1 reply matches ESC[?...c').toMatch(/^\?[\d;]*c/)
      // aterm rasterizes Sixel (+ Kitty/iTerm2), so an aterm pane's DA1 advertises
      // the Sixel bit (param 4) — apps gate inline-image support on this.
      expect(afterCsi(da1), 'aterm DA1 advertises Sixel (param 4)').toMatch(/[?;]4[;c]/)

      // 3) OSC-11 background color: the UNOPENED xterm shim has no color service
      //    so it does NOT auto-reply to OSC 10/11 — the aterm renderer (which owns
      //    the theme) answers ESC ] 11 ; rgb:RRRR/GGGG/BBBB BEL from its seeded bg.
      const osc11 = decodeCapturedReply(replies, 'OSC11')
      expect(osc11, 'captured an OSC-11 reply').not.toBeNull()
      expect(afterOsc(osc11), 'OSC-11 reply names the 11 background color').toMatch(
        /^11;rgb:[0-9a-f]{4}\/[0-9a-f]{4}\/[0-9a-f]{4}/
      )

      // 4) CSI 14t (text-area pixel size): the aterm renderer is authoritative
      //    (unopened xterm + headless daemon can't know pixel size). Reply shape
      //    is ESC [ 4 ; heightPx ; widthPx t with real (>0) device px.
      const px14 = decodeCapturedReply(replies, 'PX14')
      expect(px14, 'captured a CSI 14t reply').not.toBeNull()
      expect(afterCsi(px14), 'CSI 14t reply matches ESC[4;H;Wt').toMatch(/^4;[1-9]\d*;[1-9]\d*t/)

      // 5) CSI 16t (cell pixel size): ESC [ 6 ; cellHpx ; cellWpx t.
      const px16 = decodeCapturedReply(replies, 'PX16')
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
      rmSync(replyPath, { force: true })
    }
  })
})
