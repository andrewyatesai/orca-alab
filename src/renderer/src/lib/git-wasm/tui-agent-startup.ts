// Renderer TUI agent-startup plan builders, driven by the Rust orca-agents core
// in the orca-git wasm (the shared TS bodies were deleted). Every builder goes
// through the single `tuiAgentStartupOp` JSON boundary.
//
// Pre-ready fallback returns null (no plan). Every builder call site is
// post-mount — user interactions (composer/launch buttons, worktree nav) or
// component effects that require a hydrated store (terminal resume, worktree
// activation) — all strictly after the eager pre-mount `startGitWasm()`. Mount
// + IPC store-hydration far exceeds the local wasm compile, so the null branch
// is defensive only, never hit in practice.
import { tuiAgentStartupOp } from './orca_git_wasm.js'
import { isGitWasmReady } from './git-line-stats'
import type { AgentDraftLaunchPlan, AgentStartupPlan } from '../../../../shared/tui-agent-startup'
import type { AgentStartupShell } from '../../../../shared/tui-agent-startup-shell'
import type {
  AgentProviderSessionMetadata,
  ResumableTuiAgent
} from '../../../../shared/agent-session-resume'
import type { TuiAgent } from '../../../../shared/types'

function op<T>(fn: string, input: unknown): T | null {
  if (!isGitWasmReady()) {
    return null
  }
  return JSON.parse(tuiAgentStartupOp(fn, JSON.stringify(input))) as T | null
}

export function buildAgentStartupPlan(args: {
  agent: TuiAgent
  prompt: string
  cmdOverrides: Partial<Record<TuiAgent, string>>
  platform: NodeJS.Platform
  shell?: AgentStartupShell
  allowEmptyPromptLaunch?: boolean
  agentArgs?: string | null
  agentEnv?: Record<string, string> | null
  isRemote?: boolean
}): AgentStartupPlan | null {
  return op<AgentStartupPlan>('buildAgentStartupPlan', args)
}

export function buildAgentResumeStartupPlan(args: {
  agent: ResumableTuiAgent
  providerSession: AgentProviderSessionMetadata
  cmdOverrides: Partial<Record<TuiAgent, string>>
  platform: NodeJS.Platform
  shell?: AgentStartupShell
  agentArgs?: string | null
  agentEnv?: Record<string, string> | null
  agentCommand?: string | null
  isRemote?: boolean
}): AgentStartupPlan | null {
  return op<AgentStartupPlan>('buildAgentResumeStartupPlan', args)
}

export function buildAgentDraftLaunchPlan(args: {
  agent: TuiAgent
  draft: string
  cmdOverrides: Partial<Record<TuiAgent, string>>
  platform: NodeJS.Platform
  shell?: AgentStartupShell
  agentArgs?: string | null
  agentEnv?: Record<string, string> | null
  isRemote?: boolean
}): AgentDraftLaunchPlan | null {
  return op<AgentDraftLaunchPlan>('buildAgentDraftLaunchPlan', args)
}
