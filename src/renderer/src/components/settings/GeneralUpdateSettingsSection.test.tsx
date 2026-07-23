// @vitest-environment happy-dom
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useAppStore } from '../../store'
import { GeneralUpdateSettingsSection } from './GeneralUpdateSettingsSection'

const download = vi.fn()
const openUrl = vi.fn()

beforeEach(() => {
  useAppStore.setState(useAppStore.getInitialState(), true)
  download.mockReset().mockResolvedValue(undefined)
  openUrl.mockReset().mockResolvedValue(undefined)
  Object.defineProperty(globalThis, 'ORCA_BUILD_INFO', {
    configurable: true,
    value: {
      orcaVersion: '1.4.201',
      orcaCommit: 'abc1234',
      orcaCommitDate: '2026-07-22',
      atermRev: 'e268133',
      upstreamFork: 'stablyai/orca',
      upstreamAligned: 'unknown'
    }
  })
  Object.defineProperty(window, 'api', {
    configurable: true,
    value: {
      app: {
        getIdentity: vi.fn().mockResolvedValue({ name: 'Orca: ALab Edition' })
      },
      shell: { openUrl },
      updater: {
        check: vi.fn(),
        download,
        getVersion: vi.fn().mockResolvedValue('1.4.201'),
        quitAndInstall: vi.fn().mockResolvedValue(undefined)
      }
    }
  })
})

afterEach(() => {
  cleanup()
  useAppStore.setState(useAppStore.getInitialState(), true)
})

describe('GeneralUpdateSettingsSection manual installation', () => {
  it('shows one manual action, keeps release notes distinct, and delegates to main', () => {
    const releaseUrl = 'https://github.com/alabsystems/orca-alab/releases/tag/v1.4.201'
    useAppStore.setState({
      updateStatus: {
        state: 'available',
        version: '1.4.201',
        releaseUrl,
        installMode: 'manual',
        changelog: null
      }
    })

    render(<GeneralUpdateSettingsSection />)

    const manualActions = screen.getAllByText('Download Manually')
    expect(manualActions).toHaveLength(1)
    expect(screen.getByRole('link', { name: 'Release notes' }).getAttribute('href')).toBe(
      releaseUrl
    )

    fireEvent.click(screen.getByRole('button', { name: 'Download Manually' }))

    expect(download).toHaveBeenCalledTimes(1)
    expect(openUrl).not.toHaveBeenCalled()
  })
})
