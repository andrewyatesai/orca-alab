// TS dispatch for the setup-script-telemetry parity module: maps the shared
// vector function names to the real `src/shared/setup-script-telemetry.ts`
// exports so the harness compares the live TS reference against the Rust port.

import {
  buildSetupScriptPromptActionTelemetry,
  buildSetupScriptPromptTelemetry,
  type SetupScriptPromptAction
} from '../../../src/shared/setup-script-telemetry'
import type { SetupScriptImportCandidate } from '../../../src/shared/setup-script-imports'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'buildSetupScriptPromptTelemetry': {
      const { candidate, hasSharedHooks } = input as {
        candidate: SetupScriptImportCandidate | null
        hasSharedHooks: boolean
      }
      return buildSetupScriptPromptTelemetry({ candidate, hasSharedHooks })
    }
    case 'buildSetupScriptPromptActionTelemetry': {
      const { action, candidate, hasSharedHooks, editedBeforeSave } = input as {
        action: SetupScriptPromptAction
        candidate: SetupScriptImportCandidate | null
        hasSharedHooks: boolean
        editedBeforeSave?: boolean
      }
      return buildSetupScriptPromptActionTelemetry({
        action,
        candidate,
        hasSharedHooks,
        editedBeforeSave
      })
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
