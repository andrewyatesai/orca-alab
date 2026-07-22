import { describe, expect, it } from 'vitest'
import {
  COMPLETED_ROW_MESSAGES,
  COMPLETED_ROW_STATE,
  INITIAL_ROW_MESSAGES,
  INITIAL_ROW_STATE,
  ORCHESTRATION_BEATS,
  type RowMessages,
  type RowState
} from './orchestration-types'

describe('orchestration walkthrough storyboard', () => {
  it('orders dependencies, a blocking question, a decision, and blocker recovery', () => {
    expect(ORCHESTRATION_BEATS.map((beat) => beat.phase)).toEqual([
      'dispatch',
      'dependency',
      'question',
      'decision',
      'relay',
      'unblocked',
      'unblocked',
      'blocker',
      'recovery',
      'complete'
    ])
    expect(ORCHESTRATION_BEATS[2]).toMatchObject({
      senderMessage: 'migration-blocking-question',
      recipientMessage: 'coord-decision-gate',
      senderState: 'question'
    })
    expect(ORCHESTRATION_BEATS[3]).toMatchObject({
      actor: 'human',
      delivery: 'local',
      senderMessage: 'coord-human-decision-resolved',
      phase: 'decision'
    })
    expect(ORCHESTRATION_BEATS[4]).toMatchObject({
      actor: 'coordinator',
      senderMessage: 'coord-decision-recorded',
      recipientMessage: 'migration-applying-decision',
      phase: 'relay'
    })
    expect(ORCHESTRATION_BEATS[7]).toMatchObject({
      senderMessage: 'middleware-check-blocked',
      recipientMessage: 'coord-recovery-gate',
      senderState: 'blocked'
    })
    expect(ORCHESTRATION_BEATS[8]).toMatchObject({
      senderMessage: 'coord-recovery-plan',
      recipientMessage: 'middleware-rerunning-check'
    })
  })

  it('finishes with the coordinator and both accountable workers complete', () => {
    const result = ORCHESTRATION_BEATS.reduce(
      (snapshot, beat) => ({
        state: {
          ...snapshot.state,
          ...(beat.senderState ? { [beat.from]: beat.senderState } : {}),
          ...(beat.recipientState ? { [beat.to]: beat.recipientState } : {})
        },
        messages: {
          ...snapshot.messages,
          ...(beat.senderMessage ? { [beat.from]: beat.senderMessage } : {}),
          ...(beat.recipientMessage ? { [beat.to]: beat.recipientMessage } : {})
        }
      }),
      {
        state: { ...INITIAL_ROW_STATE } as RowState,
        messages: { ...INITIAL_ROW_MESSAGES } as RowMessages
      }
    )

    expect(result.state).toEqual(COMPLETED_ROW_STATE)
    expect(result.messages).toEqual(COMPLETED_ROW_MESSAGES)
  })
})
