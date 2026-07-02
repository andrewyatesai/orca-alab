export type TerminalWebglAutoDecision = {
  allowWebgl: boolean
  reason:
    | 'non-linux-hardware-renderer'
    | 'non-linux-renderer-unknown'
    | 'non-linux-software-renderer'
    | 'linux-wayland'
    | 'linux-hardware-renderer'
    | 'linux-webgl2-unavailable'
    | 'linux-renderer-unavailable'
    | 'linux-software-renderer'
  renderer: string | null
  vendor: string | null
}

let cachedDecision: TerminalWebglAutoDecision | null = null

// Why: software GL backends (SwiftShader/llvmpipe/Microsoft Basic Render etc.)
// are slow + buggy for terminal rendering and show up on ALL platforms — common
// inside VMs, RDP/remote-desktop, and headless runners — not just Linux. The
// generic `software` token catches "Software Rasterizer"/"Software Adapter"
// variants; `basic render`/`microsoft basic render` is the Windows WARP driver.
const SOFTWARE_RENDERER_PATTERN =
  /\b(swiftshader|llvmpipe|softpipe|software|basic render|microsoft basic render driver|virgl|svga3d)\b/i

export function resetTerminalWebglAutoDecision(): void {
  cachedDecision = null
}

export function isLinuxRendererHost(
  platform: string = typeof navigator === 'undefined' ? '' : navigator.platform,
  userAgent: string = typeof navigator === 'undefined' ? '' : navigator.userAgent
): boolean {
  if (userAgent.startsWith('Node.js/')) {
    return false
  }
  return platform.includes('Linux') || userAgent.includes('Linux')
}

function readRendererDisplayServer(): 'wayland' | 'x11' | null {
  try {
    return window.api.platform.get().displayServer
  } catch {
    return null
  }
}

function readWebglRendererInfo(): Pick<TerminalWebglAutoDecision, 'renderer' | 'vendor'> & {
  hasWebgl2: boolean
  hasRendererInfo: boolean
} {
  if (typeof document === 'undefined') {
    return { hasWebgl2: false, hasRendererInfo: false, renderer: null, vendor: null }
  }

  try {
    const canvas = document.createElement('canvas')
    const gl = canvas.getContext('webgl2')
    if (!gl) {
      return { hasWebgl2: false, hasRendererInfo: false, renderer: null, vendor: null }
    }

    const debugInfo = gl.getExtension('WEBGL_debug_renderer_info')
    if (!debugInfo) {
      return { hasWebgl2: true, hasRendererInfo: false, renderer: null, vendor: null }
    }

    const renderer = String(gl.getParameter(debugInfo.UNMASKED_RENDERER_WEBGL) ?? '')
    const vendor = String(gl.getParameter(debugInfo.UNMASKED_VENDOR_WEBGL) ?? '')
    return {
      hasWebgl2: true,
      hasRendererInfo: renderer.length > 0 || vendor.length > 0,
      renderer: renderer || null,
      vendor: vendor || null
    }
  } catch {
    return { hasWebgl2: false, hasRendererInfo: false, renderer: null, vendor: null }
  }
}

/** macOS/Windows gate: unlike Linux (where corruption can leave WebGL alive but
 *  wrong), a missing renderer string on a non-Linux host is NOT itself a reason
 *  to fall back — we only block KNOWN software backends (SwiftShader/llvmpipe/
 *  Microsoft Basic Render etc.), which run terminal rendering on a slow/buggy
 *  software GL path inside VMs/RDP/remote desktop. Hardware + unidentifiable
 *  renderers are allowed (the prior non-Linux default), so this only narrows the
 *  set that previously got an unconditional yes. */
function decideNonLinuxWebgl(): TerminalWebglAutoDecision {
  const rendererInfo = readWebglRendererInfo()
  // No renderer identity (no WebGL2 context, or no debug-renderer-info extension):
  // keep the historical non-Linux default of trying WebGL — we can't prove it's
  // software, and non-Linux hardware GL stays robust even without the debug ext.
  if (!rendererInfo.hasRendererInfo) {
    return {
      allowWebgl: true,
      reason: 'non-linux-renderer-unknown',
      renderer: rendererInfo.renderer,
      vendor: rendererInfo.vendor
    }
  }

  const identity = `${rendererInfo.vendor ?? ''} ${rendererInfo.renderer ?? ''}`
  if (SOFTWARE_RENDERER_PATTERN.test(identity)) {
    return {
      allowWebgl: false,
      reason: 'non-linux-software-renderer',
      renderer: rendererInfo.renderer,
      vendor: rendererInfo.vendor
    }
  }

  return {
    allowWebgl: true,
    reason: 'non-linux-hardware-renderer',
    renderer: rendererInfo.renderer,
    vendor: rendererInfo.vendor
  }
}

export function getTerminalWebglAutoDecision(): TerminalWebglAutoDecision {
  if (cachedDecision) {
    return cachedDecision
  }

  if (!isLinuxRendererHost()) {
    cachedDecision = decideNonLinuxWebgl()
    return cachedDecision
  }

  if (readRendererDisplayServer() === 'wayland') {
    // Why: #5319 can wedge terminal input during xterm WebGL context creation
    // on Linux Wayland before xterm reports a recoverable context-loss event.
    cachedDecision = {
      allowWebgl: false,
      reason: 'linux-wayland',
      renderer: null,
      vendor: null
    }
    return cachedDecision
  }

  const rendererInfo = readWebglRendererInfo()
  if (!rendererInfo.hasWebgl2) {
    cachedDecision = {
      allowWebgl: false,
      reason: 'linux-webgl2-unavailable',
      renderer: rendererInfo.renderer,
      vendor: rendererInfo.vendor
    }
    return cachedDecision
  }

  if (!rendererInfo.hasRendererInfo) {
    // Why: the Linux corruption path can leave WebGL alive while glyphs are bad;
    // without renderer identity we cannot distinguish hardware from software GL.
    cachedDecision = {
      allowWebgl: false,
      reason: 'linux-renderer-unavailable',
      renderer: rendererInfo.renderer,
      vendor: rendererInfo.vendor
    }
    return cachedDecision
  }

  const identity = `${rendererInfo.vendor ?? ''} ${rendererInfo.renderer ?? ''}`
  if (SOFTWARE_RENDERER_PATTERN.test(identity)) {
    cachedDecision = {
      allowWebgl: false,
      reason: 'linux-software-renderer',
      renderer: rendererInfo.renderer,
      vendor: rendererInfo.vendor
    }
    return cachedDecision
  }

  cachedDecision = {
    allowWebgl: true,
    reason: 'linux-hardware-renderer',
    renderer: rendererInfo.renderer,
    vendor: rendererInfo.vendor
  }
  return cachedDecision
}
