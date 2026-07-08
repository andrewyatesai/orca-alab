// TUI agent-startup plan TYPES + non-cutover re-exports.
//
// The plan BUILDERS (buildAgentStartupPlan / buildAgentResumeStartupPlan /
// buildAgentDraftLaunchPlan) were cut over to the Rust orca-agents core: the
// main process drives them via napi (`src/main/rust-tui-agent-startup.ts`) and
// the renderer via the orca-git wasm (`@/lib/git-wasm/tui-agent-startup`), both
// over the single `tuiAgentStartupOp` JSON boundary. This module keeps only the
// shared result types and re-exports the still-TS shell/detection helpers.
import { isShellProcess } from './agent-detection'
import type { SleepingAgentLaunchConfig } from './agent-session-resume'
import type { StartupCommandDelivery } from './codex-startup-delivery'
import type { TuiAgent } from './types'

export type AgentStartupPlan = {
  agent: TuiAgent
  launchCommand: string
  expectedProcess: string
  followupPrompt: string | null
  launchConfig: SleepingAgentLaunchConfig
  launchToken?: string
  draftPrompt?: string | null
  env?: Record<string, string>
  startupCommandDelivery?: StartupCommandDelivery
}

export type AgentDraftLaunchPlan = {
  agent: TuiAgent
  launchCommand: string
  expectedProcess: string
  launchConfig: SleepingAgentLaunchConfig
  env?: Record<string, string>
  startupCommandDelivery?: StartupCommandDelivery
}

export { isShellProcess }
export {
  buildShellCommandFromArgv,
  planAgentCliArgsSuffix,
  quoteStartupArg,
  resolveStartupShell
} from './tui-agent-startup-shell'
export type { AgentCliArgsPlan, AgentStartupShell } from './tui-agent-startup-shell'
