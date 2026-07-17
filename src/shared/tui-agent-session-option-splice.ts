// Fork seam for upstream #9085 (per-model session option pickers): the plan
// BUILDERS live in the Rust orca-agents core, but the session-option catalogs
// are TS closures (per-agent launchArgs/composeModelValue) that evolve with the
// native-chat surface. Like the hermes-query wrapper, the Rust core keeps
// producing the option-free plan (its command IS commandWithoutSessionOptions,
// which is exactly what launchConfig must snapshot), and this splice layers the
// option flags into the launch command through the same TS resolver upstream
// uses (`resolveAgentLaunchCommand`).
import { resolveAgentLaunchCommand } from './tui-agent-launch-command'
import type { SessionOptionValue } from './native-chat-session-options'
import { resolveStartupShell, type AgentStartupShell } from './tui-agent-startup-shell'
import type { TuiAgent } from './types'

type SessionOptionSpliceArgs = {
  agent: TuiAgent
  cmdOverrides: Partial<Record<TuiAgent, string>>
  platform: NodeJS.Platform
  shell?: AgentStartupShell
  agentArgs?: string | null
  sessionOptions?: Record<string, SessionOptionValue>
  isRemote?: boolean
}

/**
 * Splice resolved session-option flags into a Rust-built launch plan.
 *
 * The Rust plan's launchCommand always begins with the option-free resolved
 * command (base + user-args suffix); upstream inserts option flags between the
 * base and the user suffix so an explicit user flag stays the final, winning
 * occurrence. Replacing that prefix with upstream's `command` reproduces the
 * exact upstream launch command while every injection tail (prompt flags,
 * draft flags, env-var clears) is preserved verbatim.
 */
export function spliceSessionOptionsIntoPlan<
  T extends { launchCommand: string; sessionOptions?: Record<string, SessionOptionValue> }
>(plan: T, args: SessionOptionSpliceArgs): T | null {
  if (!args.sessionOptions || Object.keys(args.sessionOptions).length === 0) {
    return plan
  }
  const resolved = resolveAgentLaunchCommand({
    agent: args.agent,
    cmdOverrides: args.cmdOverrides,
    platform: args.platform,
    shell: resolveStartupShell(args.platform, args.shell),
    agentArgs: args.agentArgs,
    sessionOptions: args.sessionOptions,
    isRemote: args.isRemote
  })
  if (!resolved.ok) {
    return null
  }
  const applied =
    Object.keys(resolved.appliedSessionOptions).length > 0
      ? { sessionOptions: { ...resolved.appliedSessionOptions } }
      : {}
  if (resolved.command === resolved.commandWithoutSessionOptions) {
    return { ...plan, ...applied }
  }
  if (!plan.launchCommand.startsWith(resolved.commandWithoutSessionOptions)) {
    // Rust/TS command drift — surface it rather than corrupt the command; the
    // plan still launches, only without the picker flags.
    console.warn('[tui-agent-startup] session-option splice anchor mismatch', {
      agent: args.agent
    })
    return plan
  }
  return {
    ...plan,
    launchCommand:
      resolved.command + plan.launchCommand.slice(resolved.commandWithoutSessionOptions.length),
    ...applied
  }
}
