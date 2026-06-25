import type { IDisposable } from './terminal-types'

/** A typed event emitter that returns xterm-style disposables, used for the
 *  facade's onData / onResize / onBell / onSelectionChange channels. */
export type FacadeEmitter<T> = {
  /** Subscribe; returns a disposable that removes the listener. */
  on(handler: (value: T) => void): IDisposable
  /** Fire all current listeners. */
  emit(value: T): void
  /** True when at least one listener is attached. */
  hasListeners(): boolean
  /** Drop every listener (facade dispose). */
  clear(): void
}

export function createFacadeEmitter<T>(): FacadeEmitter<T> {
  const listeners = new Set<(value: T) => void>()
  return {
    on(handler) {
      listeners.add(handler)
      return { dispose: () => void listeners.delete(handler) }
    },
    emit(value) {
      listeners.forEach((listener) => listener(value))
    },
    hasListeners() {
      return listeners.size > 0
    },
    clear() {
      listeners.clear()
    }
  }
}
