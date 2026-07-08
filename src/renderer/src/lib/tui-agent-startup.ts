// Renderer hub for TUI agent-startup: the plan builders now come from the Rust
// orca-git wasm wrapper; the shell/detection helpers stay TS (re-exported from
// shared). Consumers import from here so the wasm/TS split is a single choke point.
export {
  buildAgentResumeStartupPlan,
  buildAgentDraftLaunchPlan,
  buildAgentStartupPlan
} from './git-wasm/tui-agent-startup'
export {
  planAgentCliArgsSuffix,
  isShellProcess,
  quoteStartupArg,
  resolveStartupShell
} from '../../../shared/tui-agent-startup'
export type {
  AgentCliArgsPlan,
  AgentDraftLaunchPlan,
  AgentStartupPlan
} from '../../../shared/tui-agent-startup'
