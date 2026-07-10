import { describe, expect, it } from 'vitest'
import {
  ATERM_RAIN_SIGNAL_CODES,
  EMPTY_ATERM_RAIN_PULSE_BUFFER,
  bufferAtermRainPulse,
  classifyAtermRainPulse,
  coalesceAtermRainPulse,
  resumeAtermRainPulses
} from './aterm-rain-signal'

describe('observable aterm rain signals', () => {
  it('maps real tool phases into a small stable visual vocabulary', () => {
    expect(classifyAtermRainPulse({ state: 'working', toolName: 'Read' }).signal).toBe('inspect')
    expect(classifyAtermRainPulse({ state: 'working', toolName: 'Edit' }).signal).toBe('modify')
    expect(classifyAtermRainPulse({ state: 'working', toolName: 'Bash' }).signal).toBe('execute')
    expect(classifyAtermRainPulse({ state: 'working', toolName: 'WebFetch' }).signal).toBe(
      'network'
    )
    expect(classifyAtermRainPulse({ state: 'working', toolName: 'Task' }).signal).toBe('branch')
  })

  it.each([
    ['read_file', 'inspect'],
    ['exec_command', 'execute'],
    ['shell_command', 'execute'],
    ['run_command', 'execute'],
    ['run_shell_command', 'execute'],
    ['run_terminal_cmd', 'execute'],
    ['execute_code', 'execute'],
    ['web_search', 'network'],
    ['apply_patch', 'modify']
  ] as const)('classifies normalized live hook alias %s as %s', (toolName, signal) => {
    expect(classifyAtermRainPulse({ state: 'working', toolName }).signal).toBe(signal)
  })

  it.each(['UserPromptSubmit', 'user_prompt_submit', 'turn_start', 'agent.start', 'pre_llm_call'])(
    'recognizes turn-start event %s across provider naming conventions',
    (hookEventName) => {
      expect(classifyAtermRainPulse({ state: 'working', hookEventName }).signal).toBe('turn_start')
    }
  )

  it("locks Orca's numeric half of the ABI shared with aterm RainSignal", () => {
    expect(ATERM_RAIN_SIGNAL_CODES).toEqual({
      assistant: 0,
      inspect: 1,
      modify: 2,
      execute: 3,
      network: 4,
      branch: 5,
      waiting: 6,
      success: 7,
      failure: 8,
      interrupted: 9,
      turn_start: 10
    })
  })

  it('carries no prompt, tool input, path, URL, or network payload', () => {
    const secret = 'token=never-cross-the-rain-boundary'
    const pulse = classifyAtermRainPulse({
      state: 'working',
      hookEventName: 'PreToolUse',
      toolName: 'WebSearch',
      ...({ toolInput: secret, prompt: secret } as object)
    })
    expect(pulse).toEqual({ signal: 'network', weight: 5 })
    expect(JSON.stringify(pulse)).not.toContain(secret)
  })

  it('gives interruption and failure precedence over ordinary work', () => {
    expect(classifyAtermRainPulse({ state: 'done', hookEventName: 'tool_failure' }).signal).toBe(
      'failure'
    )
    expect(classifyAtermRainPulse({ state: 'done', interrupted: true }).signal).toBe('interrupted')
  })

  it('coalesces a drawer-swap gap with the same priority rules as the engine', () => {
    expect(
      coalesceAtermRainPulse({ signal: 'failure', weight: 7 }, { signal: 'inspect', weight: 8 })
    ).toEqual({ signal: 'failure', weight: 8 })
    expect(
      coalesceAtermRainPulse({ signal: 'inspect', weight: 3 }, { signal: 'success', weight: 6 })
    ).toEqual({ signal: 'success', weight: 6 })
  })

  it('preserves the independent turn boundary beneath a stronger swap-gap outcome', () => {
    const turn = { signal: 'turn_start', weight: 4 } as const
    const failure = { signal: 'failure', weight: 7 } as const
    const failed = bufferAtermRainPulse(EMPTY_ATERM_RAIN_PULSE_BUFFER, {
      ...failure
    })
    const withTurn = bufferAtermRainPulse(failed, turn)
    expect(resumeAtermRainPulses(withTurn)).toEqual([turn, failure])

    const turnFirst = bufferAtermRainPulse(EMPTY_ATERM_RAIN_PULSE_BUFFER, turn)
    expect(resumeAtermRainPulses(turnFirst)).toEqual([turn])
    expect(resumeAtermRainPulses(bufferAtermRainPulse(turnFirst, failure))).toEqual([turn, failure])
  })
})
