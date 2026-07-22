/* oxlint-disable react-doctor/no-adjust-state-on-prop-change -- Why: this page is a timed storyboard; row state resets are part of replaying the animation when the active step changes. */
import { useCallback, useEffect, useRef, useState } from 'react'
import type { JSX } from 'react'
import {
  BUBBLE_FLIGHT_MS,
  BUBBLE_GAP_MS,
  BUBBLE_LAND_MS,
  COMPLETED_ROW_MESSAGES as DONE_COPY,
  COMPLETED_ROW_STATE as DONE_ROWS,
  INITIAL_ROW_MESSAGES as START_COPY,
  INITIAL_ROW_STATE as START_ROWS,
  ORCHESTRATION_CLI_COMMAND_TIMINGS_MS,
  ORCHESTRATION_BEATS,
  RESPONSE_BEAT_GAP_MS,
  type AgentKey,
  type Beat,
  type OrchestrationPhase,
  type RowFlash,
  type RowMessages,
  type RowPending,
  type RowState
} from './orchestration-types'
import { arrowPathFromCoordTo, bubblePathBetweenRows } from './orchestration-bubble-path'
import { OrchestrationWorkspaceCards } from './OrchestrationWorkspaceCards'

// Children start pending (no agent row visible) and reveal as the orchestrator
// dispatches a message to them. This mirrors the "agents arrive when assigned"
// reading the design wants.
const START_PENDING: RowPending = {
  'child-codex': true,
  'child-claude': true
}

const CHILD_ONE_CREATE_MS = ORCHESTRATION_CLI_COMMAND_TIMINGS_MS[0]
const CHILD_TWO_CREATE_MS = ORCHESTRATION_CLI_COMMAND_TIMINGS_MS[1]
const FIRST_DISPATCH_MS = ORCHESTRATION_CLI_COMMAND_TIMINGS_MS[2]
const FINAL_RESULT_HOLD_MS = 300

function storySettleMs(showResponseBeats: boolean): number {
  const beatCount = showResponseBeats ? ORCHESTRATION_BEATS.length : 2
  return (
    FIRST_DISPATCH_MS +
    (beatCount > 0 ? BUBBLE_GAP_MS : 0) +
    Math.max(0, beatCount - 1) * RESPONSE_BEAT_GAP_MS +
    FINAL_RESULT_HOLD_MS
  )
}

export function OrchestrationPage(props: {
  active: boolean
  reducedMotion: boolean
  onCycleComplete?: () => void
  controlledCreatedChildCount?: number
  showResponseBeats?: boolean
}): JSX.Element {
  const {
    active,
    reducedMotion,
    onCycleComplete,
    controlledCreatedChildCount,
    showResponseBeats = true
  } = props
  const stageRef = useRef<HTMLDivElement | null>(null)
  const arrowsRef = useRef<SVGSVGElement | null>(null)
  const bubbleLayerRef = useRef<HTMLDivElement | null>(null)
  const rowRefs = useRef<Partial<Record<AgentKey, HTMLDivElement | null>>>({})
  const childCountControlledRef = useRef(controlledCreatedChildCount !== undefined)

  const [rowState, setRowState] = useState<RowState>(reducedMotion ? DONE_ROWS : START_ROWS)
  const initialMessages = reducedMotion ? DONE_COPY : START_COPY
  const [rowMessages, setRowMessages] = useState<RowMessages>(initialMessages)
  const [rowFlash, setRowFlash] = useState<RowFlash>({})
  const [rowPending, setRowPending] = useState<RowPending>(reducedMotion ? {} : START_PENDING)
  const [createdChildCount, setCreatedChildCount] = useState(reducedMotion ? 2 : 0)
  const [phase, setPhase] = useState<OrchestrationPhase>(reducedMotion ? 'complete' : 'plan')
  const showSettledReducedState = active && reducedMotion
  const displayedChildCount =
    controlledCreatedChildCount ?? (showSettledReducedState ? 2 : createdChildCount)

  // Why: bubbles measure the recipient row at fire-time, so the pending flag
  // has to flip *before* the path is computed. React state updates are async,
  // so keep a synchronous mirror to flip styles immediately.
  const pendingMirror = useRef<RowPending>(reducedMotion ? {} : { ...START_PENDING })

  childCountControlledRef.current = controlledCreatedChildCount !== undefined

  const drawArrow = useCallback((): void => {
    const arrows = arrowsRef.current
    const stage = stageRef.current
    if (!arrows || !stage) {
      return
    }
    arrows.removeAttribute('data-fading')
    const stageRect = stage.getBoundingClientRect()
    arrows.setAttribute('viewBox', `0 0 ${stageRect.width} ${stageRect.height}`)
    arrows.setAttribute('width', String(stageRect.width))
    arrows.setAttribute('height', String(stageRect.height))
    const coordEl = stage.querySelector('[data-feature-wall-card="coord"]')
    if (!(coordEl instanceof HTMLElement)) {
      arrows.innerHTML = ''
      return
    }
    const codexEl = stage.querySelector('[data-feature-wall-card="child"]')
    const claudeEl = stage.querySelector('[data-feature-wall-card="child-claude"]')
    const paths: string[] = []
    if (codexEl instanceof HTMLElement) {
      paths.push(arrowPathFromCoordTo(coordEl, codexEl, stageRect))
    }
    if (claudeEl instanceof HTMLElement) {
      paths.push(arrowPathFromCoordTo(coordEl, claudeEl, stageRect))
    }
    arrows.innerHTML = paths.map((d) => `<path d="${d}"/>`).join('')
  }, [])

  useEffect(() => {
    if (active && displayedChildCount >= 2) {
      const frameId = requestAnimationFrame(() => drawArrow())
      return () => cancelAnimationFrame(frameId)
    }
    return undefined
  }, [active, displayedChildCount, drawArrow])

  useEffect(() => {
    if (!active) {
      // Reset everything to the initial state when the user pages away so
      // re-entering the step plays from the top.
      setRowState(START_ROWS)
      setRowMessages(START_COPY)
      setRowFlash({})
      setRowPending(START_PENDING)
      setCreatedChildCount(0)
      setPhase('plan')
      pendingMirror.current = { ...START_PENDING }
      const arrows = arrowsRef.current
      if (arrows) {
        arrows.innerHTML = ''
      }
      const layer = bubbleLayerRef.current
      if (layer) {
        layer.innerHTML = ''
      }
      return
    }

    if (reducedMotion) {
      // Why: the static state preserves the outcome of the dependency graph,
      // rather than stranding motion-sensitive users in its opening beat.
      setRowState(DONE_ROWS)
      setRowMessages(DONE_COPY)
      setRowPending({})
      setCreatedChildCount(2)
      setPhase('complete')
      pendingMirror.current = {}
      const frameId = requestAnimationFrame(() => drawArrow())
      return () => cancelAnimationFrame(frameId)
    }

    let cancelled = false
    const timeouts: number[] = []
    const frames = new Set<number>()
    const later = (fn: () => void, ms: number): void => {
      timeouts.push(window.setTimeout(() => !cancelled && fn(), ms))
    }
    const nextFrame = (fn: () => void): void => {
      const frameId = requestAnimationFrame(() => {
        frames.delete(frameId)
        if (!cancelled) {
          fn()
        }
      })
      frames.add(frameId)
    }

    const clearArrows = (): void => {
      const arrows = arrowsRef.current
      if (arrows) {
        arrows.innerHTML = ''
      }
    }

    const fireBubble = (beat: Beat): void => {
      const fromRow = rowRefs.current[beat.from]
      const toRow = rowRefs.current[beat.to]
      const stage = stageRef.current
      const layer = bubbleLayerRef.current
      if (!fromRow || !toRow || !stage || !layer) {
        return
      }

      const senderMessage = beat.senderMessage
      if (senderMessage !== undefined) {
        setRowMessages((messages) => ({ ...messages, [beat.from]: senderMessage }))
        setRowFlash((flash) => ({ ...flash, [beat.from]: (flash[beat.from] ?? 0) + 1 }))
      }
      const senderState = beat.senderState
      if (senderState !== undefined) {
        setRowState((state) => ({ ...state, [beat.from]: senderState }))
      }

      if (beat.delivery === 'local') {
        // Why: a decision gate is resolved by the human in Orca before the
        // coordinator is allowed to relay that decision to a worker.
        setPhase(beat.phase)
        return
      }

      // The bubble needs a real geometry target. If the recipient row is
      // still pending (collapsed), aim the bubble at its parent card center
      // — the row itself reveals only when the bubble lands.
      const wasPending = pendingMirror.current[beat.to] === true
      const targetForPath: HTMLElement = wasPending
        ? ((toRow.closest('[data-feature-wall-card]') as HTMLElement | null) ?? toRow)
        : toRow

      const path = bubblePathBetweenRows(stage, fromRow, targetForPath)
      const bubble = document.createElement('div')
      bubble.className = 'feature-wall-bubble'
      bubble.style.offsetPath = `path("${path}")`
      bubble.innerHTML =
        '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" ' +
        'stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden>' +
        '<rect x="3" y="5" width="18" height="14" rx="2"/>' +
        '<path d="M3 7l9 6 9-6"/></svg>'
      layer.appendChild(bubble)
      void bubble.offsetWidth
      nextFrame(() => bubble.classList.add('in-flight'))

      later(() => {
        // Reveal the recipient agent on landing — that's the moment work
        // "arrives" at the child workspace.
        if (wasPending) {
          pendingMirror.current = { ...pendingMirror.current, [beat.to]: false }
          setRowPending((p) => ({ ...p, [beat.to]: false }))
        }
        const recipientMessage = beat.recipientMessage
        if (recipientMessage !== undefined) {
          setRowMessages((m) => ({ ...m, [beat.to]: recipientMessage }))
          setRowFlash((f) => ({ ...f, [beat.to]: (f[beat.to] ?? 0) + 1 }))
        }
        const recipientState = beat.recipientState
        if (recipientState !== undefined) {
          setRowState((state) => ({ ...state, [beat.to]: recipientState }))
        }
        setPhase(beat.phase)
        bubble.classList.remove('in-flight')
        bubble.classList.add('landed')
      }, BUBBLE_FLIGHT_MS)

      later(() => bubble.remove(), BUBBLE_LAND_MS)
    }

    const runOnce = (done: () => void): void => {
      clearArrows()
      setRowState(START_ROWS)
      setRowMessages(START_COPY)
      setRowPending(START_PENDING)
      setCreatedChildCount(0)
      setPhase('plan')
      pendingMirror.current = { ...START_PENDING }
      if (!childCountControlledRef.current) {
        // Reveal each child workspace when the matching shell command appears,
        // so the CLI tip reads as Claude driving the exact Orca workflow shown.
        later(() => {
          setCreatedChildCount(1)
        }, CHILD_ONE_CREATE_MS)
        later(() => {
          setCreatedChildCount(2)
          later(() => drawArrow(), 360)
        }, CHILD_TWO_CREATE_MS)
      }
      const beats = showResponseBeats ? ORCHESTRATION_BEATS : ORCHESTRATION_BEATS.slice(0, 2)
      let beatIdx = 0
      const next = (): void => {
        if (beatIdx >= beats.length) {
          later(done, FINAL_RESULT_HOLD_MS)
          return
        }
        fireBubble(beats[beatIdx])
        beatIdx += 1
        later(next, beatIdx < 2 ? BUBBLE_GAP_MS : RESPONSE_BEAT_GAP_MS)
      }
      later(next, FIRST_DISPATCH_MS)
    }

    runOnce(() => onCycleComplete?.())

    const onResize = (): void => drawArrow()
    window.addEventListener('resize', onResize)

    const cleanupLayer = bubbleLayerRef.current
    return () => {
      cancelled = true
      timeouts.forEach((id) => window.clearTimeout(id))
      frames.forEach((id) => cancelAnimationFrame(id))
      frames.clear()
      window.removeEventListener('resize', onResize)
      if (cleanupLayer) {
        cleanupLayer.innerHTML = ''
      }
    }
  }, [active, onCycleComplete, reducedMotion, drawArrow, showResponseBeats])

  return (
    <div
      ref={stageRef}
      className="feature-wall-orch-stage relative grid"
      data-feature-wall-orchestration-story="true"
      data-feature-wall-story-loop="once"
      data-feature-wall-story-settle-ms={storySettleMs(showResponseBeats)}
      style={{
        gridTemplateColumns: 'minmax(0, 1fr)',
        gridAutoRows: 'min-content',
        rowGap: 28,
        paddingRight: 56,
        alignItems: 'start',
        alignContent: 'center',
        height: '100%'
      }}
    >
      <OrchestrationWorkspaceCards
        displayedChildCount={displayedChildCount}
        phase={phase}
        registerRow={(agent, node) => {
          rowRefs.current[agent] = node
        }}
        rowFlash={rowFlash}
        rowMessages={rowMessages}
        rowPending={rowPending}
        rowState={rowState}
        showRunStatus={showResponseBeats}
        showSettledReducedState={showSettledReducedState}
      />

      <svg
        ref={arrowsRef}
        className="feature-wall-orch-arrows"
        aria-hidden
        preserveAspectRatio="none"
      />
      <div
        ref={bubbleLayerRef}
        aria-hidden
        style={{ position: 'absolute', inset: 0, pointerEvents: 'none', zIndex: 3 }}
      />
    </div>
  )
}
