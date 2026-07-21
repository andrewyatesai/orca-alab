import { parseGitHubRemoteIdentity } from './github-remote-identity-parsing'

// Resolves which gh host GitHub Projects calls should target. gh defaults to
// github.com, so in multi-host setups (github.com + GHES) Projects queries hit
// the wrong host (#1715). The host is derived from the workspace repos' git
// remotes: an owner whose repos live on exactly one non-github.com GitHub host
// gets that host pinned via `--hostname`.

export type ProjectsRemoteInventoryEntry = { remoteUrl: string }

const PROJECTS_GH_HOST_CACHE_MAX_ENTRIES = 512

let readRemoteInventory: (() => ProjectsRemoteInventoryEntry[]) | null = null

// Why: the repo inventory lives on the per-process Store (main process and SSH
// runtime each have their own); a registration callback avoids importing
// persistence into this module and keeps both processes on one code path.
export function registerProjectsHostRemoteInventory(
  read: () => ProjectsRemoteInventoryEntry[]
): void {
  readRemoteInventory = read
}

// Hosts learned outside the remote inventory (e.g. a project resolved on a
// host once). Keyed by lowercase owner; bounded insertion-order eviction.
const learnedOwnerHosts = new Map<string, string>()
// Project node id → host, stamped when a table loads, so node-id-only
// mutations (field updates) can follow the project's host.
const projectHosts = new Map<string, string>()

function rememberBounded(map: Map<string, string>, key: string, value: string): void {
  if (!map.has(key) && map.size >= PROJECTS_GH_HOST_CACHE_MAX_ENTRIES) {
    const oldest = map.keys().next().value
    if (oldest !== undefined) {
      map.delete(oldest)
    }
  }
  map.set(key, value)
}

export function _resetProjectsGhHostForTests(): void {
  readRemoteInventory = null
  learnedOwnerHosts.clear()
  projectHosts.clear()
}

/** Owner → gh host derived from workspace repo remotes. Returns null when the
 *  owner resolves to github.com or is ambiguous across hosts (default gh host
 *  behavior — the pre-#1715 status quo — applies). */
export function resolveProjectsGhHost(owner: string): string | null {
  const normalizedOwner = owner.trim().toLowerCase()
  if (!normalizedOwner) {
    return null
  }
  const hosts = new Set<string>()
  for (const entry of readRemoteInventory?.() ?? []) {
    const identity = parseGitHubRemoteIdentity(entry.remoteUrl)
    if (identity && identity.owner.toLowerCase() === normalizedOwner) {
      hosts.add(identity.host)
    }
  }
  const learned = learnedOwnerHosts.get(normalizedOwner)
  if (learned) {
    hosts.add(learned)
  }
  if (hosts.size !== 1) {
    // Why: an owner seen on several hosts (or none) cannot be pinned safely;
    // fall back to gh's default host rather than guessing.
    return null
  }
  const [host] = hosts
  return host === 'github.com' ? null : host
}

export function rememberProjectsGhHostForOwner(owner: string, host: string | null): void {
  const normalizedOwner = owner.trim().toLowerCase()
  if (!normalizedOwner || !host || host === 'github.com') {
    return
  }
  rememberBounded(learnedOwnerHosts, normalizedOwner, host)
}

export function rememberProjectsGhHostForProject(projectId: string, host: string | null): void {
  if (!projectId || !host || host === 'github.com') {
    return
  }
  rememberBounded(projectHosts, projectId, host)
}

export function projectsGhHostForProject(projectId: string): string | null {
  return projectHosts.get(projectId) ?? null
}

/** gh argv fragment pinning the request host; empty for the default host. */
export function projectsGhHostArgs(host: string | null | undefined): string[] {
  return host && host !== 'github.com' ? ['--hostname', host] : []
}
