// The builder bodies were cut over to the Rust `orca_core::setup_script_telemetry`
// port (renderer drives it via the orca-git wasm; see
// src/renderer/src/lib/git-wasm/setup-script-telemetry.ts). Only the wire type
// remains here so any surface can import it without a napi/wasm dependency.
import type { EventProps } from './telemetry-events'

export type SetupScriptPromptAction = EventProps<'setup_script_prompt_action'>['action']
