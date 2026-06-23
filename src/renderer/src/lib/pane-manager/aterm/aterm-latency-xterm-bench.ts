import { buildDefaultTerminalOptions } from '../pane-terminal-options'

/** Build a real xterm Terminal + WebGL addon and time `frames` single-cell
 *  write→painted cycles — the same single-cell update on the renderer Orca
 *  replaced. xterm's WebGL renderer paints on its OWN rAF (by design), so we
 *  measure the HONEST end-to-end thing xterm exposes: the wall-clock time from a
 *  single-cell terminal.write() to the next onRender event (the addon's GPU paint
 *  completing). This INCLUDES xterm's rAF render-debounce — which is part of its
 *  real per-keystroke latency, not an artifact — and is averaged over `frames`.
 *
 *  Note this is NOT identical to the aterm `render()`+finish() number (pure render
 *  work); xterm's renderer is rAF-driven and offers no synchronous public present.
 *  The spec labels this column "write→painted (incl. rAF)" so the comparison is
 *  read honestly rather than as raw GPU-draw cost. */
export async function benchXtermWebglFrame(opts: {
  cols: number
  rows: number
  frames: number
}): Promise<number> {
  const { cols, rows, frames } = opts
  const [{ Terminal }, { WebglAddon }] = await Promise.all([
    import('@xterm/xterm'),
    import('@xterm/addon-webgl')
  ])
  // xterm's renderer is rAF-driven and skips painting a host that isn't laid out
  // in the viewport, so it must be on-screen and visible to fire onRender (an
  // off-screen left:-10000px host never paints). Keep it visually inert — pinned
  // top-left, near-transparent, behind the app, no pointer events — and removed in
  // finally. (The e2e window is hidden, so this is invisible to the user anyway.)
  const host = document.createElement('div')
  host.style.position = 'fixed'
  host.style.left = '0'
  host.style.top = '0'
  host.style.zIndex = '-1'
  host.style.opacity = '0.01'
  host.style.pointerEvents = 'none'
  host.style.width = `${cols * 12}px`
  host.style.height = `${rows * 20}px`
  document.body.appendChild(host)
  // Use the SAME terminal options the live panes use so the comparison reflects
  // the real renderer config (font, weights, contrast), then pin the grid.
  const terminal = new Terminal({ ...buildDefaultTerminalOptions(), cols, rows, cursorBlink: false })
  let webgl: InstanceType<typeof WebglAddon> | null = null
  try {
    terminal.open(host)
    webgl = new WebglAddon()
    terminal.loadAddon(webgl)

    // Resolve when xterm fires its next render (the addon has painted to the GPU),
    // but BOUND the wait: xterm's renderer can skip painting an off-screen/hidden
    // host headless, so onRender may never fire — without a cap the whole bench
    // hangs to the test timeout. A timed-out frame rejects so the caller records
    // xterm as unmeasurable in this environment rather than hanging.
    const nextRender = (): Promise<void> =>
      new Promise<void>((resolve, reject) => {
        const disposable = terminal.onRender(() => {
          clearTimeout(timer)
          disposable.dispose()
          resolve()
        })
        const timer = setTimeout(() => {
          disposable.dispose()
          reject(new Error('xterm onRender did not fire within 2000ms (off-screen/headless)'))
        }, 2000)
      })

    // Fill the screen once with dense content, then wait for it to paint.
    let filled = ''
    for (let r = 0; r < rows; r++) {
      let lineStr = `\x1b[${(r % 7) + 31}m`
      for (let c = 0; c < cols; c++) {
        lineStr += String.fromCharCode(33 + ((r * 7 + c) % 94))
      }
      filled += `${lineStr}\x1b[0m`
      if (r < rows - 1) {
        filled += '\r\n'
      }
    }
    const settled = nextRender()
    terminal.write(`\x1b[H${filled}`)
    await settled

    // Time `frames` single-cell write→painted cycles. Each iteration writes one
    // alternating glyph at home and awaits the onRender that paints it. The grid
    // changes every frame so xterm can't coalesce to a no-op.
    let total = 0
    for (let i = 0; i < frames; i++) {
      const painted = nextRender()
      const t0 = performance.now()
      terminal.write(i % 2 === 0 ? '\x1b[1;1HA' : '\x1b[1;1HB')
      await painted
      total += performance.now() - t0
    }
    return total / frames
  } finally {
    try {
      webgl?.dispose()
    } catch {
      /* ignore */
    }
    terminal.dispose()
    host.remove()
  }
}
