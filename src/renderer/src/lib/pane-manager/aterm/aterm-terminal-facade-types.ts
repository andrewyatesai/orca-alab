import type { IDisposable, ILinkProvider, IMarker, ITerminalOptions } from './terminal-types'
import type { AtermPaneController } from './aterm-pane-controller-types'
import type { AtermFacadeBuffer } from './aterm-facade-buffer'
import type { AtermFacadeParser } from './aterm-facade-parser'
import type { AtermAppNotification } from './aterm-notification-drain'

/** A live theme/option bag the facade exposes as `terminal.options`. The aterm
 *  controller reads most of these live (applyTerminalAppearance writes them here
 *  and re-themes the engine in place); the facade just stores them so reads round
 *  trip and the controller-side getters (macOptionIsMeta/cursorBlink) see them.
 *  Typed as xterm's option bag so consumers read fontSize/scrollback/cursorBlink/
 *  macOptionIsMeta with their real types. */
export type AtermFacadeOptions = ITerminalOptions

/** The xterm-`Terminal`-shaped surface orca's ~46 consumers use, backed entirely
 *  by the live aterm engine via the controller. Typed structurally (not as the
 *  real `Terminal`) so the facade can omit xterm internals orca never touches. */
export type AtermTerminalFacade = {
  readonly cols: number
  readonly rows: number
  readonly buffer: AtermFacadeBuffer
  readonly parser: AtermFacadeParser
  // undefined (not null) to match xterm's Terminal.element/textarea so the facade
  // is drop-in for Pick<Terminal,'element'> consumers.
  readonly element: HTMLElement | undefined
  readonly textarea: HTMLTextAreaElement | undefined
  options: AtermFacadeOptions
  unicode: { activeVersion: string; versions: readonly string[]; register: () => void }
  modes: {
    readonly applicationCursorKeysMode: boolean
    readonly bracketedPasteMode: boolean
    readonly mouseTrackingMode: string
    readonly sendFocusMode: boolean
  }
  write(data: string, callback?: () => void): void
  /** Internal: the output scheduler's post-mirror write. The engine was already
   *  fed up front via __feedEngine (mirrorOutputToAterm), so this only fires the
   *  parsed callback — it must NOT re-feed the engine (no double-parse). */
  __schedulerWrite(data: string, callback?: () => void): void
  /** Internal: schedule a canvas redraw of the engine's already-mirrored state.
   *  The scheduler calls this after a callback-only __schedulerWrite flush, which
   *  feeds no bytes and so schedules no draw — without it the engine can hold newer
   *  state than the last painted frame (stale canvas). Coalesced, so safe to call
   *  per flush (no double-draw storm). */
  __scheduleAtermDraw(): void
  input(data: string): void
  paste(text: string): void
  resize(cols: number, rows: number): void
  clear(): void
  reset(): void
  refresh(start?: number, end?: number): void
  focus(): void
  blur(): void
  scrollToBottom(): void
  scrollToTop(): void
  scrollToLine(line: number): void
  scrollLines(amount: number): void
  registerMarker(cursorYOffset?: number): IMarker | undefined
  loadAddon(addon: unknown): void
  attachCustomKeyEventHandler(handler: (event: KeyboardEvent) => boolean): void
  /** Internal: the consumer hook attachCustomKeyEventHandler registered (null
   *  before registration). The controller options read it live per keydown. */
  readonly __customKeyEventHandler: ((event: KeyboardEvent) => boolean) | null
  registerLinkProvider(provider: ILinkProvider): IDisposable
  getSelection(): string
  hasSelection(): boolean
  clearSelection(): void
  getSelectionPosition(): { start: { x: number; y: number }; end: { x: number; y: number } } | null
  onData(handler: (data: string) => void): IDisposable
  onResize(handler: (size: { cols: number; rows: number }) => void): IDisposable
  onTitleChange(handler: (title: string) => void): IDisposable
  onBell(handler: () => void): IDisposable
  onSelectionChange(handler: () => void): IDisposable
  /** aterm extra (no xterm equivalent): OSC 9/99/777 desktop notifications drained
   *  from the engine's fail-closed, host-authorized queue. */
  onTerminalAppNotification(handler: (notification: AtermAppNotification) => void): IDisposable
  dispose(): void
  /** Internal: bind the async-attached controller + its DOM and flush buffered
   *  process() bytes. Called by openAtermPane once the controller resolves. */
  __attachController(
    controller: AtermPaneController,
    dom: { element: HTMLElement; textarea: HTMLTextAreaElement }
  ): void
  /** Internal: feed engine bytes (the output mirror calls this). Buffers until
   *  the controller attaches, then processes live + drains side channels. */
  __feedEngine(data: string): void
}
