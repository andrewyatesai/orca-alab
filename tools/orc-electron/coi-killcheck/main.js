// Rung-2 Phase-0 kill-check (moonshot Campaign 3 §7): decide, on the STOCK
// Electron 43 binary, whether crossOriginIsolated can be reached for the app's own
// privileged scheme via COOP/COEP(credentialless) AND whether a <webview> guest still
// attaches under COEP. Outcome gates the ORC-ELECTRON FORK's origin-isolation patch:
//   - COI true + webview attaches  -> stock rung-2 suffices; the fork patch is NOT
//     required for durable SAB / wasm threads (big win: no Chromium rebuild).
//   - COI true + webview FAILS     -> the documented kill-risk is REAL; the fork's
//     origin-isolation patch (grant COI to the scheme directly, PR #50789 seam) is
//     justified — its whole point is to delete this exact webview-under-COEP failure.
//   - COI false                    -> rung-2 header path can't reach COI on this pin;
//     WebContentsView escape hatch or fork patch.
// Runs headless-ish: creates an offscreen-ish window, evaluates, writes JSON, quits.
// NO app dependencies — a scratch harness, safe to run against the installed binary.

const { app, BrowserWindow, protocol } = require('electron')
const fs = require('node:fs')
const path = require('node:path')

const OUT = path.join(__dirname, 'killcheck-result.json')
const SCHEME = 'orc'

// Register BEFORE app ready: privileged + secure + supportFetchAPI so the scheme can
// carry COOP/COEP and reach crossOriginIsolated, exactly the rung-2 serving design.
protocol.registerSchemesAsPrivileged([
  {
    scheme: SCHEME,
    privileges: {
      standard: true,
      secure: true,
      supportFetchAPI: true,
      corsEnabled: true,
      stream: true
    }
  }
])

// COEP off (COI_OFF=1) is the BASELINE run: if the webview attaches with COEP off but
// not on, the COEP-blocks-webview kill-risk is REAL and the fork patch is justified.
const COEP_OFF = process.env.COI_OFF === '1'
const COI_HEADERS = COEP_OFF
  ? {}
  : {
      'Cross-Origin-Opener-Policy': 'same-origin',
      // credentialless keeps subresources loadable without CORP on every guest.
      'Cross-Origin-Embedder-Policy': 'credentialless'
    }
// CORP on the guest so a COEP:require-corp guest could also load (belt-and-suspenders).
const GUEST_HEADERS = { 'Cross-Origin-Resource-Policy': 'same-origin', ...COI_HEADERS }

function pageHtml() {
  // Reports crossOriginIsolated + durable-SAB, then diagnoses the <webview> guest:
  // did-attach vs did-finish-load are logged SEPARATELY (attach-failure is the real
  // COEP kill-risk; load-failure with attach-ok is a lesser CORP issue on the guest).
  return `<!doctype html><meta charset="utf-8"><title>coi</title>
<body><script>
(async () => {
  const result = { crossOriginIsolated: !!self.crossOriginIsolated, sabDurable: false }
  try { const sab = new SharedArrayBuffer(8, { maxByteLength: 16 }); result.sabDurable = typeof sab.grow === 'function' } catch (e) { result.sabDurable = false; result.sabError = String(e) }
  const trace = { attached: false, finished: false, failed: null }
  try {
    const wv = document.createElement('webview')
    wv.setAttribute('src', 'orc://app/guest.html')
    wv.setAttribute('partition', 'persist:coi-killcheck')
    let settled = false
    const done = (v) => { if (settled) return; settled = true; result.webview = v; result.webviewTrace = trace; window.__done(JSON.stringify(result)) }
    wv.addEventListener('did-attach', () => { trace.attached = true })
    wv.addEventListener('did-finish-load', () => { trace.finished = true; done(trace.attached ? 'attached+loaded' : 'loaded-no-attach-event') })
    wv.addEventListener('did-fail-load', (e) => { trace.failed = e.errorCode; done((trace.attached ? 'attached' : 'no-attach') + '+load-failed:' + e.errorCode) })
    wv.addEventListener('destroyed', () => done('destroyed'))
    document.body.appendChild(wv)
    // 8s: guest attach+load can be slow on a cold offscreen window.
    setTimeout(() => done(trace.attached ? 'attached-no-load' : 'timeout-no-attach'), 8000)
  } catch (e) { result.webview = 'threw:' + String(e); result.webviewTrace = trace; window.__done(JSON.stringify(result)) }
})()
</script></body>`
}

app.whenReady().then(async () => {
  protocol.handle(SCHEME, (req) => {
    const u = new URL(req.url)
    const isGuest = u.pathname.endsWith('guest.html')
    const body = isGuest ? '<!doctype html><title>guest</title><body>guest-ok</body>' : pageHtml()
    return new Response(body, {
      headers: { 'Content-Type': 'text/html', ...(isGuest ? GUEST_HEADERS : COI_HEADERS) }
    })
  })

  const win = new BrowserWindow({
    show: false,
    width: 400,
    height: 300,
    webPreferences: {
      webviewTag: true, // exercise the guest-attach path the app relies on
      contextIsolation: true,
      nodeIntegration: false,
      preload: path.join(__dirname, 'preload.js')
    }
  })

  const finish = (result) => {
    fs.writeFileSync(
      OUT,
      JSON.stringify(
        { electron: process.versions.electron, chrome: process.versions.chrome, ...result },
        null,
        2
      )
    )
    console.log(`KILLCHECK-RESULT ${JSON.stringify(result)}`)
    setTimeout(() => app.quit(), 100)
  }

  // preload exposes window.__done; a hard timeout guarantees we always exit.
  win.webContents.ipc.on('killcheck-done', (_e, json) => finish(JSON.parse(json)))
  setTimeout(
    () =>
      finish({
        crossOriginIsolated: false,
        webview: 'harness-timeout',
        note: 'no result within 12s'
      }),
    12000
  )

  await win.loadURL(`${SCHEME}://app/index.html`)
})

app.on('window-all-closed', () => app.quit())
