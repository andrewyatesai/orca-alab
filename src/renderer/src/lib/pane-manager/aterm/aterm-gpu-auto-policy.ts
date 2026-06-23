import { useAppStore } from '@/store'
import { getTerminalWebglAutoDecision } from '../terminal-webgl-auto-policy'
import { probeAtermGpu } from './aterm-gpu-probe'

/** Why the aterm GPU draw path was (or wasn't) chosen for a pane. Mirrors the
 *  intent of `terminal-webgl-auto-policy` (the xterm WebGL gate) so ONE user
 *  preference — `terminalGpuAcceleration` — controls both renderers. */
export type AtermGpuDecision = {
  useGpu: boolean
  reason:
    | 'forced-on' // window.__atermGpuEnabled === true (tests)
    | 'forced-off' // window.__atermGpuDisabled === true (tests)
    | 'setting-on' // terminalGpuAcceleration === 'on'
    | 'setting-off' // terminalGpuAcceleration === 'off'
    | 'auto-allowed' // auto + webgl2 creatable + not a known-bad software/Linux GPU
    | 'auto-no-webgl2' // auto + no webgl2 context creatable
    | 'auto-unsafe-renderer' // auto + software/unknown GPU (same gate as xterm WebGL)
}

function readGpuAccelerationSetting(): 'auto' | 'on' | 'off' {
  // The SAME preference that gates xterm WebGL (pane-webgl-renderer); reusing it
  // means one toggle controls both the aterm GPU path and xterm's WebGL renderer.
  return useAppStore.getState().settings?.terminalGpuAcceleration ?? 'auto'
}

/** Decide whether a new aterm pane should take the WebGL2 GPU draw path.
 *
 *  Decision order (mirrors terminal-webgl-auto-policy + adds explicit overrides):
 *   1. window.__atermGpuEnabled===true forces GPU (and __atermGpuDisabled===true
 *      forces CPU) — test overrides only; they bypass the renderer-safety gate so
 *      the e2e suite can prove the GPU path even on headless software WebGL.
 *   2. The terminalGpuAcceleration user setting: 'off' → CPU; 'on' → GPU (still
 *      requires a creatable webgl2 context, else CPU fallback).
 *   3. auto: GPU only when a webgl2 context is creatable AND the GPU is not a
 *      known-bad software/Linux-context-loss case (reuses the xterm WebGL gate's
 *      renderer-string checks); otherwise CPU. CPU is always the safe fallback. */
export function decideAtermGpu(): AtermGpuDecision {
  if (typeof window !== 'undefined') {
    // Test overrides win and skip the safety gate (the GPU e2e specs run on
    // headless software WebGL, which the auto gate would otherwise reject).
    if (window.__atermGpuEnabled === true) {
      return probeAtermGpu().available
        ? { useGpu: true, reason: 'forced-on' }
        : { useGpu: false, reason: 'auto-no-webgl2' }
    }
    if (window.__atermGpuDisabled === true) {
      return { useGpu: false, reason: 'forced-off' }
    }
  }

  const setting = readGpuAccelerationSetting()
  if (setting === 'off') {
    return { useGpu: false, reason: 'setting-off' }
  }

  // Both 'on' and 'auto' still require a creatable webgl2 context — there is no
  // GPU path without one, so this is a hard prerequisite, not a policy choice.
  if (!probeAtermGpu().available) {
    return { useGpu: false, reason: 'auto-no-webgl2' }
  }

  if (setting === 'on') {
    return { useGpu: true, reason: 'setting-on' }
  }

  // auto: reuse the xterm WebGL gate — it allows non-Linux hosts and identifiable
  // Linux hardware GPUs, and rejects software/unknown renderers (where hardware
  // corruption can leave WebGL alive but rendering wrong). CPU is the fallback.
  return getTerminalWebglAutoDecision().allowWebgl
    ? { useGpu: true, reason: 'auto-allowed' }
    : { useGpu: false, reason: 'auto-unsafe-renderer' }
}

/** True when the aterm GPU draw path should be attempted for a new pane. GPU is
 *  now the DEFAULT on capable hardware (matching orca defaulting xterm to WebGL);
 *  the CPU path stays the guaranteed fallback (here when unsafe/unavailable, and
 *  at runtime on GPU init failure or webglcontextlost). */
export function isAtermGpuEnabled(): boolean {
  return decideAtermGpu().useGpu
}
