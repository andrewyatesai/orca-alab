import { describe, expect, it } from 'vitest'
import { mobileNativeChatSessionIdentity } from './mobile-native-chat-session-identity'

describe('mobileNativeChatSessionIdentity', () => {
  it('changes when the session id changes within the same tab', () => {
    expect(mobileNativeChatSessionIdentity('tab', 'session-a')).not.toBe(
      mobileNativeChatSessionIdentity('tab', 'session-b')
    )
  })

  it('changes when the tab changes for the same session', () => {
    expect(mobileNativeChatSessionIdentity('tab-a', 'session')).not.toBe(
      mobileNativeChatSessionIdentity('tab-b', 'session')
    )
  })

  it('is stable for the same tab and session (no spurious remount on reconnect)', () => {
    expect(mobileNativeChatSessionIdentity('tab', 'session')).toBe(
      mobileNativeChatSessionIdentity('tab', 'session')
    )
  })

  it('is stable across null tab/session and distinguishes an absent session', () => {
    expect(mobileNativeChatSessionIdentity(null, null)).toBe(
      mobileNativeChatSessionIdentity(null, null)
    )
    // A tab whose provider session id has not resolved yet must not read as the
    // same surface as that tab once its session id arrives.
    expect(mobileNativeChatSessionIdentity('tab', null)).not.toBe(
      mobileNativeChatSessionIdentity('tab', 'session')
    )
  })
})
