import type { AtermPaneController } from './aterm-pane-controller-types'

/** A getter for the live controller, null before the async attach completes. */
type ControllerGetter = () => AtermPaneController | null | undefined

/** The thin xterm-`FitAddon` replacement. aterm's wiring auto-fits the grid to
 *  the container via its own ResizeObserver, so the engine grid is ALWAYS the
 *  fitted size: proposeDimensions returns it and fit() is a no-op. */
export type AtermFitAddonFacade = {
  proposeDimensions(): { cols: number; rows: number } | undefined
  fit(): void
}

export function createAtermFitAddonFacade(getController: ControllerGetter): AtermFitAddonFacade {
  return {
    proposeDimensions() {
      return getController()?.gridSize()
    },
    fit() {
      /* no-op: the controller's ResizeObserver owns grid fitting (contract). */
    }
  }
}

/** The thin xterm-`SearchAddon` replacement backed by the controller's search. */
export type AtermSearchAddonFacade = {
  findNext(query: string, options?: { caseSensitive?: boolean; regex?: boolean }): void
  findPrevious(query: string, options?: { caseSensitive?: boolean; regex?: boolean }): void
}

export function createAtermSearchAddonFacade(
  getController: ControllerGetter
): AtermSearchAddonFacade {
  return {
    findNext(query, options) {
      const controller = getController()
      if (!controller) {
        return
      }
      controller.findMatches(query, options?.caseSensitive ?? false, options?.regex ?? false)
      controller.findNextMatch()
    },
    findPrevious(query, options) {
      const controller = getController()
      if (!controller) {
        return
      }
      controller.findMatches(query, options?.caseSensitive ?? false, options?.regex ?? false)
      controller.findPreviousMatch()
    }
  }
}

/** The thin xterm-`SerializeAddon` replacement backed by the engine's native
 *  serialize (aterm produces replayable ANSI). */
export type AtermSerializeAddonFacade = {
  serialize(options?: { scrollback?: number }): string
}

export function createAtermSerializeAddonFacade(
  getController: ControllerGetter
): AtermSerializeAddonFacade {
  return {
    serialize(options) {
      return getController()?.serialize(options?.scrollback) ?? ''
    }
  }
}
