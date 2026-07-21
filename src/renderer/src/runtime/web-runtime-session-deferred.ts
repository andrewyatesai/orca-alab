// Store slices use this facade as an asynchronous cycle boundary. The runtime
// module is also eagerly used by mounted UI, so it is not itself a split point.
export {
  closeWebRuntimeSessionTab,
  createWebRuntimeSessionBrowserTab,
  createWebRuntimeSessionTerminal,
  setWebRuntimeTabProps
} from './web-runtime-session'
