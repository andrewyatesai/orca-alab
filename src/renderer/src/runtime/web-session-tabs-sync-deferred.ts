// web-runtime-session imports the store, while the sync helpers are also used
// by App. This facade preserves deferred cycle-breaking without a false split.
export {
  applyFreshWebSessionTabsSnapshot,
  applyWebSessionTabsStorePatch,
  resolveHostSessionTabIdForWebSessionTab
} from './web-session-tabs-sync'
