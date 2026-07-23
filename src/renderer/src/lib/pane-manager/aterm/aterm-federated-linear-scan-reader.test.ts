import { describe, expect, it, vi } from 'vitest'
import { createFederatedLinearScanReader } from './aterm-federated-linear-scan-reader'

describe('createFederatedLinearScanReader', () => {
  it('is null on pins WITHOUT row_range_json (over-budget stays an honest empty batch)', () => {
    const engine = { base_y: 0, search_display_origin: 0, cols: 80 }
    expect(createFederatedLinearScanReader(engine, 24, 80)).toBeNull()
  })

  it('reads row text off row_range_json, mapping absolute rows to display rows', () => {
    const row_range_json = vi.fn((first: number, count: number) =>
      JSON.stringify(
        Array.from({ length: count }, (_, i) => ({
          text: `display-row-${first + i}`,
          wrapped: false,
          len: 3
        }))
      )
    )
    const engine = { base_y: 100, search_display_origin: 100, cols: 80, row_range_json }
    const reader = createFederatedLinearScanReader(engine, 24, 80)
    // oldest = search_display_origin - base_y = 0; newest+1 = base_y + rows = 124.
    expect(reader?.oldestAbsRow).toBe(0)
    expect(reader?.rowCount).toBe(124)
    // Absolute row 120 → display row 20 (absRow - base_y).
    const rows = reader?.read(120, 2)
    expect(row_range_json).toHaveBeenCalledExactlyOnceWith(20, 2)
    expect(rows).toEqual(['display-row-20', 'display-row-21'])
  })

  it('propagates a reader gap as null (row_range_json returned undefined)', () => {
    const row_range_json = vi.fn(() => undefined)
    const engine = { base_y: 0, search_display_origin: 0, cols: 80, row_range_json }
    const reader = createFederatedLinearScanReader(engine, 24, 80)
    expect(reader?.read(0, 4)).toBeNull()
  })
})
