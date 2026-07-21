import { describe, expect, it } from 'vitest'
import { appendWorkspaceCleanupItems } from './workspace-cleanup-scan-primitives'

describe('workspace cleanup scan primitives', () => {
  it('aggregates large cleanup candidate batches without hitting argument limits', () => {
    const candidateCount = 150_000
    const candidates = Array.from({ length: candidateCount }, (_, index) => index)
    const aggregated: number[] = []

    appendWorkspaceCleanupItems(aggregated, candidates)

    expect(aggregated).toHaveLength(candidateCount)
    expect(aggregated[0]).toBe(0)
    expect(aggregated[candidateCount - 1]).toBe(candidateCount - 1)
  })
})
