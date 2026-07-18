// Runs the Rung-2 Phase-0 kill-check in BOTH modes (COEP on = rung-2, COEP off =
// baseline) on the stock Electron binary and prints a verdict deciding whether the
// orc-electron FORK's origin-isolation patch is warranted on this pin.
// Usage: node tools/orc-electron/run-killcheck.mjs   (or: pnpm fork:killcheck)
import { spawn } from 'node:child_process'
import { readFileSync, existsSync, rmSync } from 'node:fs'
import { join } from 'node:path'
import { createRequire } from 'node:module'

const HERE = import.meta.dirname
const RESULT = join(HERE, 'coi-killcheck', 'killcheck-result.json')
const MAIN = join(HERE, 'coi-killcheck', 'main.js')
const electron = createRequire(import.meta.url).resolve('electron/cli.js')

function runOnce(coiOff) {
  return new Promise((resolve) => {
    if (existsSync(RESULT)) {
      rmSync(RESULT)
    }
    const child = spawn(process.execPath, [electron, MAIN], {
      env: { ...process.env, COI_OFF: coiOff ? '1' : '', ELECTRON_RUN_AS_NODE: '' },
      stdio: 'ignore'
    })
    const started = Date.now()
    const poll = setInterval(() => {
      if (existsSync(RESULT)) {
        clearInterval(poll)
        try {
          child.kill()
        } catch {}
        resolve(JSON.parse(readFileSync(RESULT, 'utf8')))
      } else if (Date.now() - started > 25000) {
        clearInterval(poll)
        try {
          child.kill()
        } catch {}
        resolve({ error: 'timeout' })
      }
    }, 500)
  })
}

const on = await runOnce(false)
const off = await runOnce(true)

// Verdict logic (see moonshot Campaign 3 §7). did-attach firing under COEP is the load-
// bearing signal — the fork's origin-isolation patch exists ONLY to delete a webview
// that CANNOT attach under COEP.
const attachedOn = on?.webviewTrace?.attached === true
const attachedOff = off?.webviewTrace?.attached === true
let verdict, rationale
if (!on?.crossOriginIsolated) {
  verdict = 'FORK-PATCH-OR-ESCAPE-HATCH'
  rationale =
    'crossOriginIsolated is FALSE on stock via rung-2 headers; need the fork origin-isolation patch or a WebContentsView escape hatch.'
} else if (attachedOn) {
  verdict = 'STOCK-RUNG-2-SUFFICES'
  rationale =
    'crossOriginIsolated TRUE and the <webview> guest ATTACHES under COEP on the stock binary — the origin-isolation fork patch is NOT required for durable SAB / wasm threads on this pin. Ship rung-2 (orca:// + COOP/COEP) instead.'
} else if (!attachedOff) {
  verdict = 'INCONCLUSIVE-WEBVIEW'
  rationale =
    'webview did not attach with COEP off either — the no-attach is a harness/env artifact, not a COEP kill. Re-test in the real app before concluding.'
} else {
  verdict = 'FORK-PATCH-JUSTIFIED'
  rationale =
    'webview attaches with COEP OFF but NOT ON — the COEP-blocks-webview kill-risk is REAL on this pin; the fork origin-isolation patch is justified.'
}

const summary = {
  electron: on?.electron,
  chrome: on?.chrome,
  coepOn: on,
  coepOff: off,
  verdict,
  rationale
}
console.log(JSON.stringify(summary, null, 2))
console.log(`\nVERDICT: ${verdict}\n${rationale}`)
