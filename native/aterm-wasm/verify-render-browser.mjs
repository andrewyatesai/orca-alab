// Proof that aterm renders in orca's actual renderer engine (Chromium): serve the
// web wasm + font, load the page in headless Chromium via Playwright, and
// screenshot the canvas aterm painted (no xterm.js involved).
import { chromium } from '@playwright/test'
import http from 'node:http'
import { readFileSync, existsSync } from 'node:fs'
import { join, extname, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const dir = join(dirname(fileURLToPath(import.meta.url)), 'browser-proof')
const types = {
  '.js': 'text/javascript',
  '.wasm': 'application/wasm',
  '.html': 'text/html',
  '.ttf': 'font/ttf'
}
const server = http.createServer((req, res) => {
  const rel = req.url === '/' ? 'index.html' : req.url.split('?')[0]
  const p = join(dir, rel)
  if (!existsSync(p)) {
    res.writeHead(404)
    res.end()
    return
  }
  res.writeHead(200, { 'content-type': types[extname(p)] || 'application/octet-stream' })
  res.end(readFileSync(p))
})
await new Promise((r) => server.listen(0, r))
const port = server.address().port

const browser = await chromium.launch()
const page = await browser.newPage()
await page.goto(`http://localhost:${port}/index.html`)
await page.waitForFunction('window.__done === true', { timeout: 20000 })
const err = await page.evaluate(() => window.__err)
if (err) {
  console.error('PAGE ERROR:\n' + err)
  await browser.close()
  server.close()
  process.exit(1)
}
const dims = await page.evaluate(() => window.__dims)
await page.locator('#c').screenshot({ path: '/tmp/aterm-in-chromium.png' })
await browser.close()
server.close()
console.log(`✅ aterm painted a terminal in Chromium ${JSON.stringify(dims)} -> /tmp/aterm-in-chromium.png`)
