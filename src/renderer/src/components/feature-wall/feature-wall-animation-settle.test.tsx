// @vitest-environment happy-dom

import { act } from 'react'
import type { JSX } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ReviewShipWorkflowVisual } from './ReviewShipWorkflowVisual'
import { TasksAnimatedVisual } from './TasksAnimatedVisual'
import { OrchestrationPage } from './agents-orchestration/OrchestrationPage'

globalThis.IS_REACT_ACT_ENVIRONMENT = true

const mountedRoots: Root[] = []

async function mount(element: JSX.Element): Promise<HTMLDivElement> {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)
  mountedRoots.push(root)
  await act(async () => root.render(element))
  return container
}

async function advance(ms: number): Promise<void> {
  await act(async () => vi.advanceTimersByTime(ms))
}

async function advanceInSteps(ms: number): Promise<void> {
  let remaining = ms
  while (remaining > 0) {
    const step = Math.min(100, remaining)
    await advance(step)
    remaining -= step
  }
}

describe('feature wall animation settling', () => {
  beforeEach(() => vi.useFakeTimers())

  afterEach(async () => {
    await act(async () => {
      for (const root of mountedRoots.splice(0)) {
        root.unmount()
      }
    })
    document.body.innerHTML = ''
    vi.useRealTimers()
  })

  it('shows task source context before one workspace-creation pass, then stays ready', async () => {
    const container = await mount(<TasksAnimatedVisual reducedMotion={false} />)
    const visual = container.querySelector<HTMLElement>('[data-feature-wall-tasks-visual]')

    expect(visual?.dataset.animationState).toBe('idle')
    expect(Number(visual?.dataset.animationDurationMs)).toBeLessThan(5_000)
    expect(visual?.querySelector('[data-feature-wall-task-provider="github"]')).not.toBeNull()
    expect(visual?.querySelector('[data-feature-wall-linked-issue-context]')).not.toBeNull()
    expect(visual?.textContent).toContain('connected for tasks')
    expect(visual?.textContent).toContain('Linked issue #1842')

    await advance(4_999)
    expect(visual?.dataset.animationState).toBe('ready')
    const settledMarkup = visual?.outerHTML

    await advance(20_000)
    expect(visual?.dataset.animationState).toBe('ready')
    expect(visual?.outerHTML).toBe(settledMarkup)
  })

  it('stages a focused hunk before Git writes, archives once, and stays settled', async () => {
    const container = await mount(<ReviewShipWorkflowVisual reducedMotion={false} />)
    const visual = container.querySelector<HTMLElement>('[data-feature-wall-review-ship-visual]')

    expect(visual?.dataset.storyPhase).toBe('compare')
    expect(Number(visual?.dataset.animationDurationMs)).toBeLessThan(5_000)

    await advance(2_400)
    expect(visual?.dataset.storyPhase).toBe('rereview')
    expect(visual?.textContent).toContain('Re-review resolved diff')

    await advance(400)
    expect(visual?.dataset.storyPhase).toBe('stage')
    expect(visual?.querySelector('[data-review-focused-hunk="staged"]')).not.toBeNull()
    expect(visual?.textContent).toContain('Stage focused hunk · src/terminal/session.ts')

    await advance(400)
    expect(visual?.dataset.storyPhase).toBe('confirm')
    expect(visual?.textContent).toContain('Confirm commit + push')

    await advance(400)
    expect(visual?.dataset.storyPhase).toBe('archive')
    const settledMarkup = visual?.outerHTML

    await advance(20_000)
    expect(visual?.dataset.storyPhase).toBe('archive')
    expect(visual?.outerHTML).toBe(settledMarkup)
  })

  it('runs the accountable orchestration story once and stays on its result', async () => {
    const onCycleComplete = vi.fn()
    const container = await mount(
      <OrchestrationPage active reducedMotion={false} onCycleComplete={onCycleComplete} />
    )
    const visual = container.querySelector<HTMLElement>('[data-feature-wall-orchestration-story]')
    const phase = (): string | undefined =>
      visual?.querySelector<HTMLElement>('[data-feature-wall-orchestration-phase]')?.dataset
        .featureWallOrchestrationPhase
    const settleMs = Number(visual?.dataset.featureWallStorySettleMs)

    expect(visual?.dataset.featureWallStoryLoop).toBe('once')
    expect(settleMs).toBeLessThan(5_000)
    expect(phase()).toBe('plan')

    await advanceInSteps(settleMs)
    expect(phase()).toBe('complete')
    expect(visual?.textContent).toContain('Coordinator accepted the recovered run')
    expect(visual?.querySelectorAll('[data-agent-state="done"]')).toHaveLength(3)
    expect(onCycleComplete).toHaveBeenCalledTimes(1)
    const settledMarkup = visual?.outerHTML

    await advance(20_000)
    expect(phase()).toBe('complete')
    expect(visual?.outerHTML).toBe(settledMarkup)
    expect(onCycleComplete).toHaveBeenCalledTimes(1)
  })
})
