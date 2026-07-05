import { beforeEach, describe, expect, it, vi } from 'vitest'

const { netFetchMock } = vi.hoisted(() => ({
  netFetchMock: vi.fn()
}))

vi.mock('electron', () => ({
  net: { fetch: netFetchMock }
}))

import { fetchNudge, versionMatchesRange, shouldApplyNudge } from './updater-nudge'

// Why: the parsing tests below exercise fetchNudge against an injected URL —
// the fork's default nudge endpoint is null (dormant), so the default-arg
// call must never hit the network.
const TEST_NUDGE_URL = 'https://releases.example/nudge.json'

describe('updater-nudge', () => {
  beforeEach(() => {
    netFetchMock.mockReset()
  })

  describe('fetchNudge', () => {
    it('stays dormant by default: no configured nudge URL means no fetch', async () => {
      await expect(fetchNudge()).resolves.toBeNull()
      expect(netFetchMock).not.toHaveBeenCalled()
    })

    it('returns a valid config for a well-formed response', async () => {
      netFetchMock.mockResolvedValue({
        ok: true,
        json: async () => ({ id: 'campaign-1', minVersion: '1.1.0', maxVersion: '1.1.19' })
      })

      const result = await fetchNudge(TEST_NUDGE_URL)
      expect(result).toEqual({ id: 'campaign-1', minVersion: '1.1.0', maxVersion: '1.1.19' })
    })

    it('returns a valid config with only maxVersion', async () => {
      netFetchMock.mockResolvedValue({
        ok: true,
        json: async () => ({ id: 'campaign-2', maxVersion: '1.1.19' })
      })

      const result = await fetchNudge(TEST_NUDGE_URL)
      expect(result).toEqual({ id: 'campaign-2', maxVersion: '1.1.19' })
    })

    it('returns null for an empty response', async () => {
      netFetchMock.mockResolvedValue({
        ok: true,
        json: async () => ({})
      })

      await expect(fetchNudge(TEST_NUDGE_URL)).resolves.toBeNull()
    })

    it('returns null for a null response', async () => {
      netFetchMock.mockResolvedValue({
        ok: true,
        json: async () => null
      })

      await expect(fetchNudge(TEST_NUDGE_URL)).resolves.toBeNull()
    })

    it('returns null on non-ok HTTP response', async () => {
      netFetchMock.mockResolvedValue({ ok: false })

      await expect(fetchNudge(TEST_NUDGE_URL)).resolves.toBeNull()
    })

    it('returns null on network error', async () => {
      netFetchMock.mockRejectedValue(new Error('network down'))

      await expect(fetchNudge(TEST_NUDGE_URL)).resolves.toBeNull()
    })

    it('trims whitespace from the campaign id', async () => {
      netFetchMock.mockResolvedValue({
        ok: true,
        json: async () => ({ id: '  campaign-1  ', minVersion: '1.0.0' })
      })

      const result = await fetchNudge(TEST_NUDGE_URL)
      expect(result?.id).toBe('campaign-1')
    })

    it('returns null when id is missing', async () => {
      netFetchMock.mockResolvedValue({
        ok: true,
        json: async () => ({ minVersion: '1.0.0' })
      })

      await expect(fetchNudge(TEST_NUDGE_URL)).resolves.toBeNull()
    })

    it('returns null when neither version endpoint is present', async () => {
      netFetchMock.mockResolvedValue({
        ok: true,
        json: async () => ({ id: 'campaign-1' })
      })

      await expect(fetchNudge(TEST_NUDGE_URL)).resolves.toBeNull()
    })

    it('returns null when minVersion is invalid', async () => {
      netFetchMock.mockResolvedValue({
        ok: true,
        json: async () => ({ id: 'campaign-1', minVersion: 'not-a-version' })
      })

      await expect(fetchNudge(TEST_NUDGE_URL)).resolves.toBeNull()
    })

    it('returns null when maxVersion is invalid', async () => {
      netFetchMock.mockResolvedValue({
        ok: true,
        json: async () => ({ id: 'campaign-1', maxVersion: 'wat' })
      })

      await expect(fetchNudge(TEST_NUDGE_URL)).resolves.toBeNull()
    })

    it('returns null when the configured range is inverted', async () => {
      netFetchMock.mockResolvedValue({
        ok: true,
        json: async () => ({
          id: 'campaign-1',
          minVersion: '1.2.0',
          maxVersion: '1.1.0'
        })
      })

      await expect(fetchNudge(TEST_NUDGE_URL)).resolves.toBeNull()
    })
  })

  describe('versionMatchesRange', () => {
    it('bounded range match', () => {
      expect(versionMatchesRange('1.1.5', { minVersion: '1.1.0', maxVersion: '1.1.19' })).toBe(true)
      expect(versionMatchesRange('1.1.0', { minVersion: '1.1.0', maxVersion: '1.1.19' })).toBe(true)
      expect(versionMatchesRange('1.1.19', { minVersion: '1.1.0', maxVersion: '1.1.19' })).toBe(
        true
      )
      expect(versionMatchesRange('1.0.9', { minVersion: '1.1.0', maxVersion: '1.1.19' })).toBe(
        false
      )
      expect(versionMatchesRange('1.2.0', { minVersion: '1.1.0', maxVersion: '1.1.19' })).toBe(
        false
      )
    })

    it('upper-only range match', () => {
      expect(versionMatchesRange('1.1.5', { maxVersion: '1.1.19' })).toBe(true)
      expect(versionMatchesRange('1.2.0', { maxVersion: '1.1.19' })).toBe(false)
    })

    it('lower-only range match', () => {
      expect(versionMatchesRange('1.1.5', { minVersion: '1.1.0' })).toBe(true)
      expect(versionMatchesRange('1.0.0', { minVersion: '1.1.0' })).toBe(false)
    })
  })

  describe('shouldApplyNudge', () => {
    const nudge = { id: 'campaign-1', minVersion: '1.0.0' }

    it('returns true when version matches and not dismissed/pending', () => {
      expect(
        shouldApplyNudge({
          nudge,
          appVersion: '1.5.0',
          pendingUpdateNudgeId: null,
          dismissedUpdateNudgeId: null
        })
      ).toBe(true)
    })

    it('returns false when campaign already dismissed', () => {
      expect(
        shouldApplyNudge({
          nudge,
          appVersion: '1.5.0',
          pendingUpdateNudgeId: null,
          dismissedUpdateNudgeId: 'campaign-1'
        })
      ).toBe(false)
    })

    it('returns false when campaign is already pending', () => {
      expect(
        shouldApplyNudge({
          nudge,
          appVersion: '1.5.0',
          pendingUpdateNudgeId: 'campaign-1',
          dismissedUpdateNudgeId: null
        })
      ).toBe(false)
    })

    it('returns false when version does not match', () => {
      expect(
        shouldApplyNudge({
          nudge,
          appVersion: '0.9.0',
          pendingUpdateNudgeId: null,
          dismissedUpdateNudgeId: null
        })
      ).toBe(false)
    })
  })
})
