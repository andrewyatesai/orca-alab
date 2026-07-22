import { afterEach, describe, expect, it } from 'vitest'
import {
  _resetQueryReplyAuthorityStateForTest,
  clearQueryReplyAuthorityForPty,
  getQueryReplyAuthorityForPty,
  isQueryReplyAuthorityForThisView,
  setQueryReplyAuthorityForPty
} from './query-reply-authority-state'

describe('query-reply-authority-state (#9156)', () => {
  afterEach(() => _resetQueryReplyAuthorityStateForTest())

  it('fails open when no verdict exists (pre-election and pre-#9156 hosts)', () => {
    expect(isQueryReplyAuthorityForThisView('pty-1', null)).toBe(true)
    expect(isQueryReplyAuthorityForThisView('pty-1', 'viewer-A')).toBe(true)
    expect(getQueryReplyAuthorityForPty('pty-1')).toBeNull()
  })

  it('host-renderer verdict: only the host view answers', () => {
    setQueryReplyAuthorityForPty('pty-1', { kind: 'host-renderer' })
    expect(isQueryReplyAuthorityForThisView('pty-1', null)).toBe(true)
    expect(isQueryReplyAuthorityForThisView('pty-1', 'viewer-A')).toBe(false)
  })

  it('remote-viewer verdict: only the named viewer answers', () => {
    setQueryReplyAuthorityForPty('pty-1', { kind: 'remote-viewer', clientId: 'viewer-A' })
    expect(isQueryReplyAuthorityForThisView('pty-1', 'viewer-A')).toBe(true)
    expect(isQueryReplyAuthorityForThisView('pty-1', 'viewer-B')).toBe(false)
    expect(isQueryReplyAuthorityForThisView('pty-1', null)).toBe(false)
  })

  it('mobile and model verdicts silence every desktop view', () => {
    setQueryReplyAuthorityForPty('pty-1', { kind: 'mobile', clientId: 'phone-A' })
    expect(isQueryReplyAuthorityForThisView('pty-1', null)).toBe(false)
    expect(isQueryReplyAuthorityForThisView('pty-1', 'viewer-A')).toBe(false)

    setQueryReplyAuthorityForPty('pty-1', { kind: 'model' })
    expect(isQueryReplyAuthorityForThisView('pty-1', null)).toBe(false)
    expect(isQueryReplyAuthorityForThisView('pty-1', 'viewer-A')).toBe(false)
  })

  it('clear restores the fail-open default (stream end / resubscribe)', () => {
    setQueryReplyAuthorityForPty('pty-1', { kind: 'model' })
    clearQueryReplyAuthorityForPty('pty-1')
    expect(isQueryReplyAuthorityForThisView('pty-1', 'viewer-A')).toBe(true)
  })
})
