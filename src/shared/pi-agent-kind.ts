/**
 * Pi-compatible agent kinds. Both Pi and OMP (omp.sh) consume the same
 * `PI_CODING_AGENT_DIR` env contract and the same extension API, but each
 * defaults its on-disk config dir to a different `~/.<kind>/agent` path.
 * Orca's managed extension installer needs to know which agent is being
 * launched so it targets the user's actual source dir for THAT agent, with no
 * cross-agent fallback
 * (otherwise switching agents in the same workspace silently shadows the
 * other agent's user extensions).
 */
export type PiAgentKind = 'pi' | 'omp'

// The launch-command detector (detectPiAgentKindFromCommand) moved to the Rust
// orca-text core: the main process drives it via napi
// (src/main/pi/rust-pi-agent-kind.ts), the relay via wasm
// (src/relay/git-wasm.ts). This shared module keeps only the type + the
// agent-type predicate below.

/**
 * True when `agentType` names a Pi-compatible (goal/mission) kind. These agents
 * emit milestone `agent_end` events between steps while still working, so they
 * are treated differently from agents that only signal completion at turn end.
 */
export function isPiCompatibleAgentType(
  agentType: string | null | undefined
): agentType is PiAgentKind {
  return agentType === 'pi' || agentType === 'omp'
}
