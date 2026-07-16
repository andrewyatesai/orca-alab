// @vitest-environment happy-dom

import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { AtermGpuDecision } from '@/lib/pane-manager/aterm/aterm-gpu-auto-policy'

// Proves the settings pane's renderer-status row reports the SAME decision the
// pane wiring consults (aterm-gpu-auto-policy) instead of a hardcoded claim.

const mocks = vi.hoisted(() => ({
  decideAtermGpu: vi.fn<() => { useGpu: boolean; reason: string }>(),
  probeAtermGpu:
    vi.fn<() => { available: boolean; renderer: string | null; vendor: string | null }>()
}))

vi.mock('@/lib/pane-manager/aterm/aterm-gpu-auto-policy', () => ({
  decideAtermGpu: mocks.decideAtermGpu
}))
vi.mock('@/lib/pane-manager/aterm/aterm-gpu-probe', () => ({
  probeAtermGpu: mocks.probeAtermGpu
}))

import { readTerminalEngineRendererStatus } from './terminal-engine-renderer-status'

function decide(useGpu: boolean, reason: AtermGpuDecision['reason']): void {
  mocks.decideAtermGpu.mockReturnValue({ useGpu, reason })
}

beforeEach(() => {
  mocks.decideAtermGpu.mockReset()
  mocks.probeAtermGpu.mockReset().mockReturnValue({
    available: true,
    renderer: 'ANGLE (Apple, ANGLE Metal Renderer: Apple M2, Unspecified Version)',
    vendor: 'Google Inc. (Apple)'
  })
  delete window.__atermWorkerRender
})

describe('readTerminalEngineRendererStatus', () => {
  it('reports the GPU path with the probed adapter string', () => {
    decide(true, 'auto-allowed')

    const status = readTerminalEngineRendererStatus()

    expect(status.path).toBe('gpu')
    expect(status.reason).toBe('auto-allowed')
    expect(status.adapter).toBe(
      'ANGLE (Apple, ANGLE Metal Renderer: Apple M2, Unspecified Version)'
    )
  })

  it('reports the CPU path without an adapter, even when a GPU probe exists', () => {
    decide(false, 'auto-unsafe-renderer')

    const status = readTerminalEngineRendererStatus()

    expect(status.path).toBe('cpu')
    expect(status.reason).toBe('auto-unsafe-renderer')
    expect(status.adapter).toBeNull()
  })

  it('reflects the worker-presentation default and its explicit opt-out', () => {
    decide(true, 'auto-allowed')

    expect(readTerminalEngineRendererStatus().workerPresentation).toBe(true)

    window.__atermWorkerRender = false
    expect(readTerminalEngineRendererStatus().workerPresentation).toBe(false)

    window.__atermWorkerRender = true
    expect(readTerminalEngineRendererStatus().workerPresentation).toBe(true)
  })
})
