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
import {
  resolveStartupShell,
  type AgentStartupShell
} from '../../../../shared/tui-agent-startup-shell'
import { planHermesStartupQuery } from '../../../../shared/hermes-startup-query'
import { spliceSessionOptionsIntoPlan } from '../../../../shared/tui-agent-session-option-splice'
import type { SessionOptionValue } from '../../../../shared/native-chat-session-options'
import type {
  AgentProviderSessionMetadata,
  ResumableTuiAgent
} from '../../../../shared/agent-session-resume'
import { resolveCustomAgentBaseCommand } from '../../../../shared/custom-agent-profile'
import { buildPersonalizedAgentPrompt } from '../../../../shared/agent-personalization'
import type { CustomAgentProfile, TuiAgent } from '../../../../shared/types'

function op<T>(fn: string, input: unknown): T | null {
  if (!isGitWasmReady()) {
    return null
  }
  return JSON.parse(tuiAgentStartupOp(fn, JSON.stringify(input))) as T | null
}

// Why: the Rust core has no custom-profile concept (upstream #1479). Injecting
// the resolved profile command (env prefix + command) as the per-agent
// cmdOverride reproduces upstream's "profile replaces catalog cmd + override"
// exactly — the core already prefers cmdOverrides[agent] over the catalog. The
// profile's command/env fully describe the launch, so base-agent default
// args/env are suppressed.
function withCustomProfileOverride<
  A extends {
    agent: TuiAgent
    cmdOverrides: Partial<Record<TuiAgent, string>>
    platform: NodeJS.Platform
    agentArgs?: string | null
    agentEnv?: Record<string, string> | null
  }
>(args: A, customProfile: CustomAgentProfile | null): A {
  if (!customProfile) {
    return args
  }
  return {
    ...args,
    cmdOverrides: {
      ...args.cmdOverrides,
      [args.agent]: resolveCustomAgentBaseCommand(customProfile, args.platform)
    },
    agentArgs: null,
    agentEnv: null
  }
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
  sessionOptions?: Record<string, SessionOptionValue>
  isRemote?: boolean
  customProfile?: CustomAgentProfile | null
  personalizationPrompt?: string | null
}): AgentStartupPlan | null {
  const { customProfile = null, personalizationPrompt = null, ...rest } = args
  // Why: upstream #1776 wraps the task prompt with custom instructions inside
  // the builder; the Rust core only quotes/injects, so pre-wrap here.
  const opArgs = withCustomProfileOverride(
    {
      ...rest,
      prompt: buildPersonalizedAgentPrompt({ prompt: rest.prompt, personalizationPrompt })
    },
    customProfile
  )
  const plan = op<AgentStartupPlan>('buildAgentStartupPlan', opArgs)
  // Hermes owns readiness/submission via `chat --query` + a startup-query env
  // var, not stdin-after-start. The Rust core resolves the base command +
  // launchConfig; rebuild the launch here through the tested TS query builder
  // (the single source, also used by the main-process pty providers).
  if (plan && opArgs.agent === 'hermes' && opArgs.prompt.trim()) {
    // planHermesStartupQuery needs the base command and the configured args
    // SEPARATELY (it reorders them around the `chat` subcommand), so re-resolve
    // the base (the Rust core folds the arg suffix into its base command). A
    // present-but-empty override is falsy and falls through to the launch cmd.
    const baseCommand = opArgs.cmdOverrides.hermes?.trim() || 'hermes --tui'
    const queryPlan = planHermesStartupQuery({
      baseCommand,
      agentArgs: opArgs.agentArgs,
      prompt: opArgs.prompt.trim(),
      agentEnv: opArgs.agentEnv,
      platform: opArgs.platform,
      shell: resolveStartupShell(opArgs.platform, opArgs.shell),
      isRemote: opArgs.isRemote
    })
    if (!queryPlan) {
      return null
    }
    return {
      ...plan,
      launchCommand: queryPlan.command,
      followupPrompt: null,
      env: queryPlan.env,
      launchConfig: { ...plan.launchConfig, agentCommand: baseCommand }
    }
  }
  // Layer the native-chat session-option flags (upstream #9085) over the Rust
  // plan; a no-catalog agent or absent options is an identity pass.
  return plan ? spliceSessionOptionsIntoPlan(plan, opArgs) : plan
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
  /** Accepted for call-site parity; resume restores the provider session's own
   * state, so picker flags are never replayed (matches upstream). */
  sessionOptions?: Record<string, SessionOptionValue>
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
  sessionOptions?: Record<string, SessionOptionValue>
  isRemote?: boolean
  customProfile?: CustomAgentProfile | null
  personalizationPrompt?: string | null
}): AgentDraftLaunchPlan | null {
  const { customProfile = null, personalizationPrompt = null, ...rest } = args
  const opArgs = withCustomProfileOverride(
    { ...rest, draft: buildPersonalizedAgentPrompt({ prompt: rest.draft, personalizationPrompt }) },
    customProfile
  )
  const plan = op<AgentDraftLaunchPlan>('buildAgentDraftLaunchPlan', opArgs)
  return plan ? spliceSessionOptionsIntoPlan(plan, opArgs) : plan
}
