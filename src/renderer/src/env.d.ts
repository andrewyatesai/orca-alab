/// <reference types="vite/client" />

import type { PaneManager } from '@/lib/pane-manager/pane-manager'
import type { OnboardingFeatureSetupDeps } from '@/components/onboarding/onboarding-feature-setup'
import type { AtermGpuCpuCompareResult } from '@/lib/pane-manager/aterm/aterm-gpu-cpu-compare'
import type { AtermGpuCpuBenchResult } from '@/lib/pane-manager/aterm/aterm-gpu-cpu-bench'
import type { AtermLatencyBenchResult } from '@/lib/pane-manager/aterm/aterm-latency-bench'
import type { AtermMemoryBenchResult } from '@/lib/pane-manager/aterm/aterm-memory-bench'
import type { languages } from 'monaco-editor'

declare module 'monaco-editor/esm/vs/basic-languages/python/python.js' {
  export const conf: languages.LanguageConfiguration
  export const language: languages.IMonarchLanguage
}

declare global {
  var MonacoEnvironment:
    | {
        getWorker(workerId: string, label: string): Worker
      }
    | undefined
  // oxlint-disable-next-line typescript-eslint/consistent-type-definitions -- declaration merging requires interface
  interface Window {
    __paneManagers?: Map<string, PaneManager>
    __onboardingFeatureSetupDeps?: OnboardingFeatureSetupDeps
    // e2e/dev override to force the in-page aterm canvas renderer on.
    __atermRendererEnabled?: boolean
    // e2e override to force the aterm renderer OFF (existing suite asserts via
    // the xterm DOM). __atermRendererEnabled (explicit ON) takes precedence.
    __atermRendererDisabled?: boolean
    // e2e/dev override that FORCES the aterm WebGL2 GPU draw path on, bypassing
    // the auto-safety gate (so the GPU specs run even on headless software WebGL).
    // Still requires a creatable webgl2 context. See aterm-gpu-auto-policy.
    __atermGpuEnabled?: boolean
    // e2e/dev override that FORCES the aterm CPU draw path on (skips the GPU path
    // even on capable hardware). Takes precedence over the user setting + auto.
    __atermGpuDisabled?: boolean
    // e2e only: the WebGL adapter/backend string the GPU drawer acquired.
    __atermGpuAdapterInfo?: string
    // e2e only: why the GPU draw path fell back to CPU (init/surface/adapter
    // failure message), so the WebGL spec can report the cause explicitly.
    __atermGpuFailureReason?: string
    // e2e only: build a fresh CPU + GPU engine at a given grid, feed the same
    // bytes, render both offscreen, and return the per-channel max diff — the
    // browser GPU==CPU parity proof. Resolves available:false if WebGL is absent.
    __atermGpuVsCpuCompare?: (
      bytesAsLatin1: string,
      rows: number,
      cols: number
    ) => Promise<AtermGpuCpuCompareResult>
    // e2e only: GPU-vs-CPU per-frame draw-time benchmark. Builds fresh CPU + GPU
    // engines at each grid, fills dense SGR content, and times N frames per path
    // (CPU = render+blit, GPU = the full WebGL2 present). Reports steady-state
    // ms/frame + the GPU atlas warm-up cost separately.
    __atermGpuCpuBench?: (
      sizes: [number, number][],
      frames: number
    ) => Promise<AtermGpuCpuBenchResult>
    // e2e only: keystroke render-latency benchmark. Times single-cell
    // process→render→present (median/p95) for the aterm CPU + GPU paths and a
    // head-to-head per-frame table vs a real off-screen xterm + WebGL addon.
    __atermLatencyBench?: (
      sizes: [number, number][],
      iterations: number,
      warmup: number,
      frames: number
    ) => Promise<AtermLatencyBenchResult>
    // e2e only: per-pane wasm memory footprint (grid + scrollback + framebuffer +
    // atlas) averaged over `panes` live engines; fonts are deduped, so excluded.
    __atermMemoryBench?: (
      cols: number,
      rows: number,
      scrollbackLines: number,
      panes: number
    ) => Promise<AtermMemoryBenchResult>
    // e2e only: resolves the configured terminal theme bg through the real
    // pipeline (independent of what the renderer painted) for theme assertions.
    __resolveAtermThemeBg?: () => [number, number, number]
  }
}

// oxlint-disable-next-line typescript-eslint/consistent-type-definitions -- declaration merging requires interface
interface ImportMetaEnv {
  readonly VITE_EXPOSE_STORE?: boolean
  readonly VITE_ATERM_RENDERER?: string
}

export {}
