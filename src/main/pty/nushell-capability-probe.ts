import { execFile } from 'node:child_process'
import { NUSHELL_INTEGRATION_MIN_VERSION } from '../../shared/nushell-shell'

/**
 * Process-lifetime capability memo for nu's shell-integration floor (#8928 §2),
 * mirroring the GitCapabilityCache philosophy: probe once, never re-run a known
 * answer. Serves LOCAL spawns only — WSL version-checks in-distro and SSH
 * injects nothing, so this cache must never answer for other hosts.
 */

const NU_VERSION_PROBE_TIMEOUT_MS = 5_000

// Cache key = executable path (per-path isolation: /usr/bin/nu and a cargo nu can differ).
const resolvedSupport = new Map<string, boolean>()
const inFlightProbes = new Map<string, Promise<boolean>>()

/** Sync read used at spawn time; `undefined` = never probed (spawn plain -l). */
export function getCachedNushellIntegrationSupport(shellPath: string): boolean | undefined {
  return resolvedSupport.get(shellPath)
}

/** True when the `--version` output meets the integration floor. Exposed for the WSL in-distro gate parity tests. */
export function nushellVersionSupportsIntegration(versionOutput: string): boolean {
  // Why: take only the leading numeric token — a future "0.104.0 (abc)" line must not fail the compare (Critic note 3).
  const token = versionOutput.trim().split(/\r?\n/)[0]?.trim().split(/\s+/)[0] ?? ''
  const version = parseVersionTriple(token)
  if (!version) {
    return false
  }
  const floor = parseVersionTriple(NUSHELL_INTEGRATION_MIN_VERSION)!
  for (let i = 0; i < 3; i++) {
    if (version[i] !== floor[i]) {
      return version[i] > floor[i]
    }
  }
  return true
}

function parseVersionTriple(token: string): [number, number, number] | null {
  const match = /^(\d+)\.(\d+)(?:\.(\d+))?/.exec(token)
  if (!match) {
    return null
  }
  return [Number(match[1]), Number(match[2]), Number(match[3] ?? '0')]
}

function runNuVersionCommand(shellPath: string): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(
      shellPath,
      ['--version'],
      { timeout: NU_VERSION_PROBE_TIMEOUT_MS, windowsHide: true },
      (error, stdout) => {
        if (error) {
          reject(error)
          return
        }
        resolve(stdout)
      }
    )
  })
}

/**
 * Probe `<shellPath> --version` once; concurrent calls coalesce onto one child
 * process. Failures cache `false` (conservative-first: a below-floor or broken
 * nu must never receive `-e "source …"`).
 */
export function probeNushellIntegrationSupport(
  shellPath: string,
  options: { runVersionCommand?: (shellPath: string) => Promise<string> } = {}
): Promise<boolean> {
  const cached = resolvedSupport.get(shellPath)
  if (cached !== undefined) {
    return Promise.resolve(cached)
  }
  const inFlight = inFlightProbes.get(shellPath)
  if (inFlight) {
    return inFlight
  }
  const run = options.runVersionCommand ?? runNuVersionCommand
  const probe = run(shellPath)
    .then((output) => nushellVersionSupportsIntegration(output))
    .catch(() => false)
    .then((supported) => {
      resolvedSupport.set(shellPath, supported)
      inFlightProbes.delete(shellPath)
      return supported
    })
  inFlightProbes.set(shellPath, probe)
  return probe
}

/** @internal - tests need clean cache state between cases. */
export function __resetNushellCapabilityProbeCache(): void {
  resolvedSupport.clear()
  inFlightProbes.clear()
}

/** @internal - lets launch-config tests prime the spawn-time sync read without spawning nu. */
export function __seedNushellIntegrationSupport(shellPath: string, supported: boolean): void {
  resolvedSupport.set(shellPath, supported)
}
