import type { PiAgentKind } from '../../shared/pi-agent-kind'
import { requireRustGitBinding } from '../daemon/rust-git-addon'

/** Which Pi-compatible agent a launch command starts ('omp' for OMP, else
 *  'pi') — the Rust orca-text detector via napi. The relay runs the same core
 *  via wasm; the shared TS regex was deleted. */
export function detectPiAgentKindFromCommand(command: string | undefined): PiAgentKind {
  return requireRustGitBinding().detectPiAgentKindFromCommand(command) as PiAgentKind
}
