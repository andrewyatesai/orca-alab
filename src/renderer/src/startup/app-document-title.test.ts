import { describe, expect, it } from 'vitest'
import { applyAppDocumentTitle } from './app-document-title'

describe('applyAppDocumentTitle', () => {
  it('uses the exact runtime application identity', async () => {
    const target = { title: 'Orca: ALab Edition' }

    await expect(
      applyAppDocumentTitle(() => Promise.resolve({ name: 'Orca: ALab Edition' }), target)
    ).resolves.toBe(true)
    expect(target.title).toBe('Orca: ALab Edition')
  })

  it('keeps the static fallback when the identity is empty or unavailable', async () => {
    const target = { title: 'Orca: ALab Edition' }

    await expect(
      applyAppDocumentTitle(() => Promise.resolve({ name: '  ' }), target)
    ).resolves.toBe(false)
    await expect(
      applyAppDocumentTitle(() => Promise.reject(new Error('IPC unavailable')), target)
    ).resolves.toBe(false)
    expect(target.title).toBe('Orca: ALab Edition')
  })
})
