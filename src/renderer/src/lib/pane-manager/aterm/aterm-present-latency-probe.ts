/** e2e-only keystroke->present latency probe for the SHIPPED aterm renderer.
 *
 *  Correlates the engine's CONTENT GENERATION (bumped once per PTY `process()`
 *  call — the exact moment a keystroke's echo mutates the grid) with the wall
 *  time of the frame that ACTUALLY presented that content (recorded at the tail
 *  of a real canvas draw, after the CPU putImageData blit or the GPU
 *  `render()`). A spec records T0 right before dispatching a keydown, waits for
 *  the echoed marker to appear in the grid, reads the content gen G at that
 *  moment, then finds the first present with gen >= G — attributing the
 *  keystroke to the real present, not a coarse grid poll.
 *
 *  ALL work here is gated on `window.__ORCA_LATENCY_PROBE`, so when the flag is
 *  off (production, and every spec that doesn't opt in) both hooks are a single
 *  boolean check and nothing else runs on the render/process hot paths. */

/** Present-log entry: the present wall time and the content gen it drew. */
export type AtermPresentSample = {
  /** performance.now() at the tail of the real canvas present. */
  t: number
  /** window.__atermContentGen at draw time — the content this frame showed. */
  gen: number
}

type AtermLatencyProbeWindow = {
  __ORCA_LATENCY_PROBE?: boolean
  __atermContentGen?: number
  __atermPresentLog?: AtermPresentSample[]
}

// Bounded ring: a long-lived pane presents thousands of frames, so cap the log
// and drop the oldest entries. A 40-60 keystroke sample fits easily.
const PRESENT_LOG_CAP = 8192

// The probe is in-process only (the render worker has no window/canvas of its
// own to correlate). Guard so importing this from any worker-side path no-ops.
function probeWindow(): AtermLatencyProbeWindow | null {
  return typeof window === 'undefined' ? null : (window as unknown as AtermLatencyProbeWindow)
}

/** `process()`-boundary hook: advance the content generation so the next present
 *  records "this frame reflects content up to gen N". Call AFTER the engine has
 *  ingested the PTY chunk. No-op unless the probe flag is on. */
export function bumpAtermContentGen(): void {
  const w = probeWindow()
  if (!w?.__ORCA_LATENCY_PROBE) {
    return
  }
  w.__atermContentGen = (w.__atermContentGen ?? 0) + 1
}

/** Present-tail hook: record { present time, content gen just drawn }. Call ONLY
 *  after a frame actually blitted (never on a coalesced no-op early return), so
 *  the log holds real presents only. No-op unless the probe flag is on. */
export function recordAtermPresent(): void {
  const w = probeWindow()
  if (!w?.__ORCA_LATENCY_PROBE) {
    return
  }
  const log = (w.__atermPresentLog ??= [])
  log.push({ t: performance.now(), gen: w.__atermContentGen ?? 0 })
  if (log.length > PRESENT_LOG_CAP) {
    // Trim from the front so the ring stays bounded without reallocating often.
    log.splice(0, log.length - PRESENT_LOG_CAP)
  }
}
