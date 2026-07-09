import { describe, expect, it } from 'vitest'
import { CONTEXTUAL_TOUR_IDS } from './contextual-tours'
import { FEATURE_EDUCATION_CONTEXTUAL_TOUR_IDS } from './feature-education-telemetry'

// The normalize* behavior moved to the Rust orca-config core; parity vectors
// (tools/parity/vectors/feature-education-telemetry.json) own it now. Only the
// kept DATA const alignment stays under test here.
describe('feature education telemetry constants', () => {
  it('keeps contextual tour telemetry ids aligned with tour definitions', () => {
    expect(FEATURE_EDUCATION_CONTEXTUAL_TOUR_IDS).toEqual(CONTEXTUAL_TOUR_IDS)
  })
})
