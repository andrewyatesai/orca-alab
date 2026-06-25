/** Canonical terminal interface types for orca's aterm-backed terminal.
 *
 *  These replace the terminal type imports the app used while it ran on
 *  xterm.js. They model the SAME structural shapes the consumers relied on
 *  (buffer cells/lines, parser handlers, theme, options, links, markers) so the
 *  swap is mechanical, but they are owned here and backed by the aterm engine
 *  via the facade (see aterm-terminal-facade.ts / aterm-facade-buffer.ts /
 *  aterm-facade-parser.ts). Names are kept identical to the former xterm types
 *  (IDisposable, ITheme, IBufferLine, …) so consumers only change the import. */

/** A handle that can be torn down. */
export type IDisposable = {
  dispose(): void
}

/** A persistent anchor on a buffer line that survives reflow. */
export type IMarker = IDisposable & {
  readonly id: number
  /** Absolute buffer line index, or -1 once disposed. */
  readonly line: number
  readonly isDisposed: boolean
  onDispose(listener: () => void): IDisposable
}

/** A 1-based cell position within the buffer. */
export type IBufferCellPosition = {
  x: number
  y: number
}

/** A start/end range of buffer cells. */
export type IBufferRange = {
  start: IBufferCellPosition
  end: IBufferCellPosition
}

/** A single grid cell. orca's link/selection translation only reads
 *  getChars()/getWidth(); the remaining SGR accessors exist for shape parity. */
export type IBufferCell = {
  getWidth(): number
  getChars(): string
  getCode(): number
  getFgColorMode(): number
  getBgColorMode(): number
  getFgColor(): number
  getBgColor(): number
  isBold(): number
  isItalic(): number
  isDim(): number
  isUnderline(): number
  isBlink(): number
  isInverse(): number
  isInvisible(): number
  isStrikethrough(): number
  isOverline(): number
  isAttributeDefault(): boolean
  isFgRGB(): boolean
  isBgRGB(): boolean
  isFgPalette(): boolean
  isBgPalette(): boolean
  isFgDefault(): boolean
  isBgDefault(): boolean
  getUnderlineColorMode(): number
  getUnderlineColor(): number
  isUnderlineColorRGB(): boolean
  isUnderlineColorPalette(): boolean
  isUnderlineColorDefault(): boolean
  getUnderlineStyle(): number
  attributesEquals(cell: IBufferCell): boolean
}

/** A single grid line. */
export type IBufferLine = {
  readonly isWrapped: boolean
  readonly length: number
  getCell(x: number, cell?: IBufferCell): IBufferCell | undefined
  translateToString(trimRight?: boolean, startColumn?: number, endColumn?: number): string
}

/** Identifies an escape-sequence handler (prefix/intermediates/final). */
export type IFunctionIdentifier = {
  prefix?: string
  intermediates?: string
  final: string
}

/** Escape-sequence handler registration. Under aterm the engine owns CSI/ESC/DCS
 *  replies; only OSC handlers are dispatched (see aterm-facade-parser.ts). */
export type IParser = {
  registerCsiHandler(
    id: IFunctionIdentifier,
    callback: (params: (number | number[])[]) => boolean | Promise<boolean>
  ): IDisposable
  registerDcsHandler(
    id: IFunctionIdentifier,
    callback: (data: string, params: (number | number[])[]) => boolean | Promise<boolean>
  ): IDisposable
  registerEscHandler(
    id: IFunctionIdentifier,
    handler: () => boolean | Promise<boolean>
  ): IDisposable
  registerOscHandler(
    ident: number,
    callback: (data: string) => boolean | Promise<boolean>
  ): IDisposable
}

/** Hover decoration hints for a detected link. */
export type ILinkDecorations = {
  pointerCursor: boolean
  underline: boolean
}

/** A detected link over a buffer range. */
export type ILink = {
  range: IBufferRange
  text: string
  decorations?: ILinkDecorations
  activate(event: MouseEvent, text: string): void
  hover?(event: MouseEvent, text: string): void
  leave?(event: MouseEvent, text: string): void
  dispose?(): void
}

/** Provides links for a buffer line. */
export type ILinkProvider = {
  provideLinks(bufferLineNumber: number, callback: (links: ILink[] | undefined) => void): void
}

/** Option-bag link handler (terminal.options.linkHandler). */
export type ILinkHandler = {
  activate(event: MouseEvent, text: string, range: IBufferRange): void
  hover?(event: MouseEvent, text: string, range: IBufferRange): void
  leave?(event: MouseEvent, text: string, range: IBufferRange): void
  allowNonHttpProtocols?: boolean
}

/** Terminal color theme. Mirrors the xterm ITheme field set orca's themes use. */
export type ITheme = {
  foreground?: string
  background?: string
  cursor?: string
  cursorAccent?: string
  selectionBackground?: string
  selectionForeground?: string
  selectionInactiveBackground?: string
  scrollbarSliderBackground?: string
  scrollbarSliderHoverBackground?: string
  scrollbarSliderActiveBackground?: string
  overviewRulerBorder?: string
  black?: string
  red?: string
  green?: string
  yellow?: string
  blue?: string
  magenta?: string
  cyan?: string
  white?: string
  brightBlack?: string
  brightRed?: string
  brightGreen?: string
  brightYellow?: string
  brightBlue?: string
  brightMagenta?: string
  brightCyan?: string
  brightWhite?: string
  extendedAnsi?: string[]
}

/** Slim scrollbar gutter options. */
export type IScrollbarOptions = {
  width?: number
}

/** VT extension toggles orca advertises (kitty keyboard handshake). */
export type IVtExtensions = {
  kittyKeyboard?: boolean
}

/** ConPTY backend hints for native-Windows wrap heuristics. */
export type IWindowsPtyOptions = {
  backend?: 'conpty' | 'winpty'
  buildNumber?: number
}

/** CSS font-weight value accepted by the option bag. */
export type FontWeight =
  | 'normal'
  | 'bold'
  | '100'
  | '200'
  | '300'
  | '400'
  | '500'
  | '600'
  | '700'
  | '800'
  | '900'
  | number

/** The live terminal option bag (terminal.options). Models the subset orca
 *  reads/writes; the aterm controller reads theme/font/cursor/etc. live. */
export type ITerminalOptions = {
  allowProposedApi?: boolean
  allowTransparency?: boolean
  cursorBlink?: boolean
  cursorStyle?: 'block' | 'underline' | 'bar'
  cursorInactiveStyle?: 'outline' | 'block' | 'bar' | 'underline' | 'none'
  drawBoldTextInBrightColors?: boolean
  fastScrollSensitivity?: number
  fontFamily?: string
  fontSize?: number
  fontWeight?: FontWeight
  fontWeightBold?: FontWeight
  ignoreBracketedPasteMode?: boolean
  lineHeight?: number
  linkHandler?: ILinkHandler | null
  macOptionIsMeta?: boolean
  macOptionClickForcesSelection?: boolean
  minimumContrastRatio?: number
  scrollback?: number
  scrollbar?: IScrollbarOptions
  scrollSensitivity?: number
  theme?: ITheme
  vtExtensions?: IVtExtensions
  windowsPty?: IWindowsPtyOptions
}

/** The terminal surface orca's consumers use, backed by the aterm engine. The
 *  concrete shape lives on the facade; `Terminal` is its public alias so the
 *  former xterm `Terminal` type sites only change the import path. */
export type { AtermTerminalFacade as Terminal } from './aterm-terminal-facade'
