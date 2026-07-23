import { useCallback, useLayoutEffect, useRef, useState } from 'react'

const SCROLL_END_TOLERANCE_PX = 4

export function useFeatureWallTourPanelScroll(args: {
  activeStepId: string
  prefersReducedMotion: boolean
}): {
  panelRef: React.RefObject<HTMLElement | null>
  contentRef: React.RefObject<HTMLDivElement | null>
  hasMoreContent: boolean
  handleScroll: () => void
  scrollForward: () => void
} {
  const panelRef = useRef<HTMLElement>(null)
  const contentRef = useRef<HTMLDivElement>(null)
  const [hasMoreContent, setHasMoreContent] = useState(false)

  const measureOverflow = useCallback((): void => {
    const panel = panelRef.current
    if (!panel) {
      return
    }
    const remaining = panel.scrollHeight - panel.clientHeight - panel.scrollTop
    setHasMoreContent(remaining > SCROLL_END_TOLERANCE_PX)
  }, [])

  useLayoutEffect(() => {
    const panel = panelRef.current
    const content = contentRef.current
    if (!panel || !content) {
      return
    }

    // Why: each screen should introduce its story from the top, even after the
    // previous screen was scrolled to its result.
    panel.scrollTop = 0
    const frame = window.requestAnimationFrame(measureOverflow)
    const observer =
      typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measureOverflow)
    observer?.observe(panel)
    observer?.observe(content)
    window.addEventListener('resize', measureOverflow)
    return () => {
      window.cancelAnimationFrame(frame)
      observer?.disconnect()
      window.removeEventListener('resize', measureOverflow)
    }
  }, [args.activeStepId, measureOverflow])

  const scrollForward = useCallback((): void => {
    const panel = panelRef.current
    if (!panel) {
      return
    }
    panel.scrollBy({
      top: Math.max(200, panel.clientHeight * 0.75),
      behavior: args.prefersReducedMotion ? 'auto' : 'smooth'
    })
    // Why: the affordance disappears at the end; keep keyboard focus on the
    // scroll region instead of dropping it when its button unmounts.
    panel.focus({ preventScroll: true })
  }, [args.prefersReducedMotion])

  return {
    panelRef,
    contentRef,
    hasMoreContent,
    handleScroll: measureOverflow,
    scrollForward
  }
}
