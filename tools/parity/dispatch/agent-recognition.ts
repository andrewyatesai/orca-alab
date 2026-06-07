// TS dispatch for the agent-recognition parity module: maps the shared vector
// function names to the real `src/shared/agent-name-token-match.ts` and
// `agent-process-recognition.ts` exports so the harness compares the live TS
// reference against the Rust port (orca-core::agent_recognition).

import {
  titleHasAgentName,
  titleHasAnyLegacyAgentName
} from '../../../src/shared/agent-name-token-match'
import { isExpectedAgentProcess } from '../../../src/shared/agent-process-recognition'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'titleHasAgentName': {
      const { title, name } = input as { title: string; name: string }
      return titleHasAgentName(title, name)
    }
    case 'titleHasAnyLegacyAgentName': {
      const { title } = input as { title: string }
      return titleHasAnyLegacyAgentName(title)
    }
    case 'isExpectedAgentProcess': {
      const { processName, expectedProcess } = input as {
        processName: string | null | undefined
        expectedProcess: string
      }
      return isExpectedAgentProcess(processName, expectedProcess)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
