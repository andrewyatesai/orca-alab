import type { AtermPaneController } from './aterm-pane-controller-types'

type StableControllerOverrides = Pick<
  AtermPaneController,
  'process' | 'noteMatrixRainPulse' | 'onSelectionMutation' | 'element' | 'textarea' | 'dispose'
>

type StableControllerOptions = StableControllerOverrides & {
  current: () => AtermPaneController
}

/** Delegate the full controller surface through a getter so callers retain one
 * identity while context-loss recovery replaces the underlying drawer. */
export function createStableAtermPaneController(
  options: StableControllerOptions
): AtermPaneController {
  const current = options.current
  return {
    process: options.process,
    noteMatrixRainPulse: options.noteMatrixRainPulse,
    displayOffset: () => current().displayOffset(),
    scrollLines: (delta) => current().scrollLines(delta),
    scrollToBottom: () => current().scrollToBottom(),
    scrollToTop: () => current().scrollToTop(),
    scrollToLine: (line) => current().scrollToLine(line),
    selectionText: () => current().selectionText(),
    clearSelection: () => current().clearSelection(),
    selectionRange: () => current().selectionRange(),
    restoreSelectionRange: (range) => current().restoreSelectionRange(range),
    linkAt: (row, col) => current().linkAt(row, col),
    findMatches: (query, caseSensitive, isRegex) =>
      current().findMatches(query, caseSensitive, isRegex),
    findMatchesAsync: (query, caseSensitive, isRegex) =>
      current().findMatchesAsync(query, caseSensitive, isRegex),
    findNextMatch: () => current().findNextMatch(),
    findPreviousMatch: () => current().findPreviousMatch(),
    clearSearch: () => current().clearSearch(),
    searchMatchCount: () => current().searchMatchCount(),
    searchActiveMatchIndex: () => current().searchActiveMatchIndex(),
    searchResultsStale: () => current().searchResultsStale(),
    searchResultsIncomplete: () => current().searchResultsIncomplete(),
    searchIsPending: () => current().searchIsPending(),
    searchMarkerModel: () => current().searchMarkerModel(),
    onSearchStateChange: (handler) => current().onSearchStateChange(handler),
    searchActiveMatchRect: () => current().searchActiveMatchRect(),
    setFileLinkOpener: (fn) => current().setFileLinkOpener(fn),
    setUrlLinkContext: (context) => current().setUrlLinkContext(context),
    setLinkProviderSource: (source) => current().setLinkProviderSource(source),
    resetLinkHoverCache: () => current().resetLinkHoverCache(),
    bindSpillPaneKey: (paneKey) => current().bindSpillPaneKey(paneKey),
    onSelectionMutation: options.onSelectionMutation,
    updateTheme: (colors) => current().updateTheme(colors),
    setSelectionInactive: (inactive) => current().setSelectionInactive(inactive),
    setSelectionInactiveBg: (bg) => current().setSelectionInactiveBg(bg),
    reapplyEngineSettings: () => current().reapplyEngineSettings(),
    scheduleDraw: () => current().scheduleDraw(),
    onEngineSideChannel: (handler) => current().onEngineSideChannel?.(handler),
    settle: () => current().settle(),
    keyboardModeBits: () => current().keyboardModeBits(),
    encodeKeyForHost: (key, mods) => current().encodeKeyForHost(key, mods),
    rendererKind: () => current().rendererKind(),
    adapterInfo: () => current().adapterInfo(),
    setDrawSuspended: (suspended) => current().setDrawSuspended(suspended),
    lastMouseReport: () => current().lastMouseReport(),
    serialize: (scrollbackRows) => current().serialize(scrollbackRows),
    serializeScrollback: (maxRows) => current().serializeScrollback(maxRows),
    serializeAsync: (scrollbackRows) => current().serializeAsync(scrollbackRows),
    serializeScrollbackAsync: (maxRows) => current().serializeScrollbackAsync(maxRows),
    title: () => current().title(),
    onTitleChange: (handler) => current().onTitleChange(handler),
    gridSize: () => current().gridSize(),
    resize: (cols, rows) => current().resize(cols, rows),
    fitToContainer: () => current().fitToContainer(),
    isAltScreen: () => current().isAltScreen(),
    bracketedPasteMode: () => current().bracketedPasteMode(),
    setClipboardWriteAuthorized: (allowed) => current().setClipboardWriteAuthorized(allowed),
    setNotificationsAuthorized: (allowed) => current().setNotificationsAuthorized(allowed),
    setHyperlinkSchemeAuthorized: (scheme) => current().setHyperlinkSchemeAuthorized(scheme),
    element: options.element,
    textarea: options.textarea,
    isFocusEventMode: () => current().isFocusEventMode(),
    isMouseTracking: () => current().isMouseTracking(),
    isColorSchemeUpdatesMode: () => current().isColorSchemeUpdatesMode(),
    isAppCursorMode: () => current().isAppCursorMode(),
    cursorX: () => current().cursorX(),
    cursorY: () => current().cursorY(),
    cursorStyle: () => current().cursorStyle(),
    cursorHidden: () => current().cursorHidden(),
    cellSizeCss: () => current().cellSizeCss(),
    isReady: () => current().isReady(),
    baseY: () => current().baseY(),
    displayOriginAbsolute: () => current().displayOriginAbsolute(),
    rowIsWrapped: (row) => current().rowIsWrapped(row),
    rowLen: (row) => current().rowLen(row),
    rowText: (row) => current().rowText(row),
    cellText: (row, col) => current().cellText(row, col),
    cellIsWide: (row, col) => current().cellIsWide(row, col),
    drainBell: () => current().drainBell(),
    takeOscEvents: () => current().takeOscEvents(),
    takeNotifications: () => current().takeNotifications(),
    pixelSize: () => current().pixelSize(),
    themeColors: () => current().themeColors(),
    ...(current().benchmarkRender
      ? {
          benchmarkRender: (cols, rows, frames) => current().benchmarkRender!(cols, rows, frames)
        }
      : {}),
    dispose: options.dispose
  }
}
