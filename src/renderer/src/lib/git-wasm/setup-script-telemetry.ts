// Renderer setup-script prompt telemetry builders, driven by the Rust
// `orca_core::setup_script_telemetry` port in the orca-git wasm (the shared TS
// bodies were deleted). Every build goes through the single `op` JSON boundary.
// Pre-ready returns null so a telemetry payload is dropped (never mis-emitted)
// during the ~tens-of-ms wasm boot window; each call site guards the null so
// track() is never handed a null payload.
import { isGitWasmReady } from './git-line-stats'
import { orcaDispatch } from './orca_git_wasm.js'
import type { EventProps } from '../../../../shared/telemetry-events'
import type { SetupScriptImportCandidate } from '../../../../shared/setup-script-imports'
import type { SetupScriptPromptAction } from '../../../../shared/setup-script-telemetry'

type SetupScriptPromptTelemetry = Omit<EventProps<'setup_script_prompt_shown'>, 'nth_repo_added'>
type SetupScriptPromptActionTelemetry = Omit<
  EventProps<'setup_script_prompt_action'>,
  'nth_repo_added'
>

function op(fn: string, input: unknown): unknown | null {
  if (!isGitWasmReady()) {
    return null
  }
  return JSON.parse(orcaDispatch('setup-script-telemetry', fn, JSON.stringify(input ?? null)))
}

export function buildSetupScriptPromptTelemetry(args: {
  candidate: SetupScriptImportCandidate | null
  hasSharedHooks: boolean
}): SetupScriptPromptTelemetry | null {
  return op('buildSetupScriptPromptTelemetry', args) as SetupScriptPromptTelemetry | null
}

export function buildSetupScriptPromptActionTelemetry(args: {
  action: SetupScriptPromptAction
  candidate: SetupScriptImportCandidate | null
  hasSharedHooks: boolean
  editedBeforeSave?: boolean
}): SetupScriptPromptActionTelemetry | null {
  return op(
    'buildSetupScriptPromptActionTelemetry',
    args
  ) as SetupScriptPromptActionTelemetry | null
}
