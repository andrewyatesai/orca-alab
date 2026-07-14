import React, { useEffect, useRef, useState } from 'react'
import { MIN_COLUMN_WIDTH } from './column-widths'
import { translate } from '@/i18n/i18n'

type Props = {
  fieldId: string
  nextFieldId: string
  currentWidth: number
  nextWidth: number
  /** Live drag feedback — mutate the grid template via a CSS variable without
   *  a React state write. Fires at most once per animation frame. */
  onPreview: (fieldId: string, width: number, nextFieldId: string, nextWidth: number) => void
  /** Commit the final widths to state + localStorage. Fires once on mouse-up. */
  onCommit: (fieldId: string, width: number, nextFieldId: string, nextWidth: number) => void
}

// Why: stored widths are `fr` weights, not pixels — that's what keeps the
// grid fitting its container exactly. Drag math has to happen in pixels (the
// mouse moves in pixels), so we measure the rendered widths of the two
// adjacent cells at drag start, compute the new pixel split, then convert
// back to fr weights with the pair's total weight held constant. Net effect:
// dragging redistributes width between the pair without changing the grid's
// total — the table never grows.
//
// Why the preview/commit split + rAF: routing every raw mousemove through
// React state re-rendered the whole project row set per pointer frame (and
// re-read/wrote localStorage each tick). Instead we drive the live width via a
// CSS variable during the drag (coalesced to one update per frame) and only
// commit to state once, on mouse-up.
export default function ColumnResizeHandle({
  fieldId,
  nextFieldId,
  currentWidth,
  nextWidth,
  onPreview,
  onCommit
}: Props): React.JSX.Element {
  const [dragging, setDragging] = useState(false)
  const handleRef = useRef<HTMLDivElement | null>(null)
  const dragRef = useRef<{
    startX: number
    startPxA: number
    startPxB: number
    totalFr: number
  } | null>(null)
  // Why: the latest computed fr split, held so mouse-up commits the final value
  // even though the last preview may still be pending in a queued frame.
  const latestRef = useRef<{ width: number; nextWidth: number } | null>(null)
  const frameRef = useRef<number | null>(null)

  useEffect(() => {
    if (!dragging) {
      return
    }
    const onMove = (e: MouseEvent): void => {
      const drag = dragRef.current
      if (!drag) {
        return
      }
      const totalPx = drag.startPxA + drag.startPxB
      if (totalPx <= 0) {
        return
      }
      const proposedPxA = drag.startPxA + (e.clientX - drag.startX)
      const newPxA = Math.max(MIN_COLUMN_WIDTH, Math.min(totalPx - MIN_COLUMN_WIDTH, proposedPxA))
      const newFrA = (drag.totalFr * newPxA) / totalPx
      const newFrB = drag.totalFr - newFrA
      latestRef.current = { width: newFrA, nextWidth: newFrB }
      if (frameRef.current !== null) {
        return
      }
      frameRef.current = window.requestAnimationFrame(() => {
        frameRef.current = null
        const latest = latestRef.current
        if (latest) {
          onPreview(fieldId, latest.width, nextFieldId, latest.nextWidth)
        }
      })
    }
    const onUp = (): void => {
      if (frameRef.current !== null) {
        cancelAnimationFrame(frameRef.current)
        frameRef.current = null
      }
      const latest = latestRef.current
      dragRef.current = null
      latestRef.current = null
      setDragging(false)
      if (latest) {
        onCommit(fieldId, latest.width, nextFieldId, latest.nextWidth)
      }
    }
    document.addEventListener('mousemove', onMove)
    document.addEventListener('mouseup', onUp)
    const prevCursor = document.body.style.cursor
    const prevSelect = document.body.style.userSelect
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
    return () => {
      document.removeEventListener('mousemove', onMove)
      document.removeEventListener('mouseup', onUp)
      if (frameRef.current !== null) {
        cancelAnimationFrame(frameRef.current)
        frameRef.current = null
      }
      document.body.style.cursor = prevCursor
      document.body.style.userSelect = prevSelect
    }
  }, [dragging, fieldId, nextFieldId, onPreview, onCommit])

  return (
    <div
      ref={handleRef}
      role="separator"
      aria-orientation="vertical"
      aria-label={translate(
        'auto.components.github.project.ColumnResizeHandle.1304289353',
        'Resize column'
      )}
      onMouseDown={(e) => {
        if (e.button !== 0) {
          return
        }
        const cell = handleRef.current?.parentElement
        const nextCell = cell?.nextElementSibling as HTMLElement | null
        if (!cell || !nextCell) {
          return
        }
        e.preventDefault()
        e.stopPropagation()
        dragRef.current = {
          startX: e.clientX,
          startPxA: cell.offsetWidth,
          startPxB: nextCell.offsetWidth,
          totalFr: currentWidth + nextWidth
        }
        latestRef.current = null
        setDragging(true)
      }}
      onClick={(e) => e.stopPropagation()}
      onDoubleClick={(e) => {
        e.preventDefault()
        e.stopPropagation()
      }}
      style={{
        position: 'absolute',
        right: '-6px',
        top: 0,
        height: '100%',
        width: '12px',
        cursor: 'col-resize',
        userSelect: 'none',
        zIndex: 30,
        background: dragging ? 'rgba(59,130,246,0.25)' : 'transparent'
      }}
      onMouseEnter={(e) => {
        ;(e.currentTarget as HTMLDivElement).style.background = 'rgba(59,130,246,0.25)'
      }}
      onMouseLeave={(e) => {
        if (!dragging) {
          ;(e.currentTarget as HTMLDivElement).style.background = 'transparent'
        }
      }}
    />
  )
}
