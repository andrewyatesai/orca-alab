/** Stable identity for the active native-chat surface: the tab plus its resolved
 *  provider session. Used to remount MobileNativeChatView on a session/tab switch
 *  (so scroll offset, atBottom, expanded tools, and a stale send-failed banner
 *  reset instead of bleeding across sessions) and to reset the streaming-text
 *  throttle so the previous session's trailing frame cannot render over the new
 *  one. Deliberately excludes the terminal handle: a reconnect/handle flap within
 *  the same session must not remount or drop mid-stream state. */
export function mobileNativeChatSessionIdentity(
  tabId: string | null,
  sessionId: string | null
): string {
  return `${tabId ?? ''}\0${sessionId ?? ''}`
}
