import { drainAtermReplies } from './aterm-reply-drain'

// Push the host OS color scheme (light/dark) into the engine so apps that enable DEC
// mode 2031 receive the unsolicited CSI ?997 update on theme changes, and DSR 996
// queries answer correctly. The renderer's matchMedia reflects Electron's nativeTheme
// (driven by the user's theme setting), so it is the effective-appearance signal.

const DARK_QUERY = '(prefers-color-scheme: dark)'

type ColorSchemeSink = {
  set_color_scheme: (dark: boolean) => void
  take_response: () => Uint8Array | undefined
}

/** Seed the engine's color scheme + keep it synced to the OS appearance. On the worker
 *  path `set_color_scheme` posts a command and the worker drains the CSI ?997 reply to
 *  the reply channel, so the post-call drain here is a harmless no-op (the worker term's
 *  `take_response` is a stub); in-process it forwards the reply straight to the PTY. */
export function attachAtermColorSchemeSync(deps: {
  term: ColorSchemeSink
  inputSink: (data: string) => void
  isDisposed: () => boolean
}): { dispose: () => void } {
  const { term, inputSink, isDisposed } = deps
  if (typeof window.matchMedia !== 'function') {
    return { dispose: () => undefined }
  }
  const mql = window.matchMedia(DARK_QUERY)
  const push = (dark: boolean): void => {
    if (isDisposed()) {
      return
    }
    term.set_color_scheme(dark)
    drainAtermReplies(term, inputSink)
  }
  // Seed so DSR 996 answers correctly + a later 2031 subscription gets the right push.
  push(mql.matches)
  const onChange = (event: MediaQueryListEvent): void => push(event.matches)
  mql.addEventListener('change', onChange)
  return {
    dispose: () => mql.removeEventListener('change', onChange)
  }
}
