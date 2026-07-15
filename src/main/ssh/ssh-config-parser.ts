import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { homedir } from 'node:os'
import type { SshTarget } from '../../shared/ssh-types'
import { expandSshConfigIncludes } from './ssh-config-include-expander'
import { requireRustGitBinding } from '../daemon/rust-git-addon'
export { parseSshGOutput, resolveWithSshG, type SshResolvedConfig } from './ssh-g-config-resolution'

export type SshConfigHost = {
  host: string
  hostname?: string
  port?: number
  user?: string
  identityFile?: string
  identityAgent?: string
  identitiesOnly?: boolean
  gssapiAuthentication?: boolean
  proxyCommand?: string
  proxyUseFdpass?: boolean
  proxyJump?: string
}

/**
 * Parse an OpenSSH config file into structured host entries.
 * Handles Host blocks with single or multiple patterns.
 * Ignores wildcard-only patterns (e.g. "Host *").
 *
 * The parsing moved to the Rust orca-ssh core (napi); this is the only process
 * that reads ~/.ssh/config. `~` in identity paths expands against the caller's
 * home, which the pure Rust port takes explicitly.
 */
export function parseSshConfig(content: string): SshConfigHost[] {
  return JSON.parse(
    requireRustGitBinding().parseSshConfig(content, homedir())
  ) as SshConfigHost[]
}

/** Read and parse the user's ~/.ssh/config file. Returns empty array if not found. */
export function loadUserSshConfig(): SshConfigHost[] {
  const configPath = join(homedir(), '.ssh', 'config')
  if (!existsSync(configPath)) {
    return []
  }

  try {
    const content = expandSshConfigIncludes(configPath)
    return parseSshConfig(content)
  } catch {
    console.warn(`[ssh] Failed to read SSH config at ${configPath}`)
    return []
  }
}

/** Convert parsed SSH config hosts into SshTarget objects for import. */
export function sshConfigHostsToTargets(
  hosts: SshConfigHost[],
  existingTargetHosts: Set<string>
): SshTarget[] {
  const targets: SshTarget[] = []
  const seenLabels = new Set(existingTargetHosts)

  for (const entry of hosts) {
    const effectiveHost = entry.hostname || entry.host
    const label = entry.host

    if (seenLabels.has(label)) {
      continue
    }
    seenLabels.add(label)

    targets.push({
      id: `ssh-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      label,
      configHost: entry.host,
      host: effectiveHost,
      port: entry.port ?? 22,
      username: entry.user ?? '',
      identityFile: entry.identityFile,
      identityAgent: entry.identityAgent,
      identitiesOnly: entry.identitiesOnly,
      gssapiAuthentication: entry.gssapiAuthentication,
      proxyCommand: entry.proxyCommand,
      jumpHost: entry.proxyJump
    })
  }

  return targets
}
