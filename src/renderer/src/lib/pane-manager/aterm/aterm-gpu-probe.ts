/** Probe for the aterm WebGL2 GPU draw path. Cheap, synchronous, run BEFORE we
 *  commit a canvas to webgl2 (a canvas can only ever hold one context kind, so we
 *  must decide first). Creates a throwaway canvas, asks for `webgl2`, and reads
 *  the UNMASKED renderer string for logging / the e2e proof.
 *
 *  This probe only answers "is a webgl2 context CREATABLE?" — it does NOT decide
 *  software-vs-hardware. The software/Linux-context-loss safety gate (shared with
 *  xterm WebGL) lives in `aterm-gpu-auto-policy`, which reuses
 *  `terminal-webgl-auto-policy`'s renderer-string checks. The `__atermGpuEnabled`
 *  test override deliberately bypasses that gate (so the GPU specs prove the path
 *  even on headless software WebGL); auto-default uses the full gate. */
export type AtermGpuProbeResult = {
  /** True when a `webgl2` context could be created (the GPU path is attemptable). */
  available: boolean
  /** UNMASKED_RENDERER_WEBGL, when the debug-renderer extension is present. */
  renderer: string | null
  /** UNMASKED_VENDOR_WEBGL, when available. */
  vendor: string | null
}

let cached: AtermGpuProbeResult | null = null

export function resetAtermGpuProbe(): void {
  cached = null
}

export function probeAtermGpu(): AtermGpuProbeResult {
  if (cached) {
    return cached
  }
  if (typeof document === 'undefined') {
    cached = { available: false, renderer: null, vendor: null }
    return cached
  }
  try {
    const canvas = document.createElement('canvas')
    const gl = canvas.getContext('webgl2')
    if (!gl) {
      cached = { available: false, renderer: null, vendor: null }
      return cached
    }
    const debugInfo = gl.getExtension('WEBGL_debug_renderer_info')
    const renderer = debugInfo
      ? String(gl.getParameter(debugInfo.UNMASKED_RENDERER_WEBGL) ?? '') || null
      : null
    const vendor = debugInfo
      ? String(gl.getParameter(debugInfo.UNMASKED_VENDOR_WEBGL) ?? '') || null
      : null
    // Release the probe context promptly so it doesn't count against the browser's
    // live-WebGL-context budget while panes acquire their own.
    gl.getExtension('WEBGL_lose_context')?.loseContext()
    cached = { available: true, renderer, vendor }
    return cached
  } catch {
    cached = { available: false, renderer: null, vendor: null }
    return cached
  }
}
