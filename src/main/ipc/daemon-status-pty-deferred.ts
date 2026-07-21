// A dedicated dynamic-import boundary keeps daemon-status lightweight in
// isolated consumers while making clear that pty.ts is not a code-split target
// in the full main-process graph.
export { getLocalPtyProvider } from './pty'
