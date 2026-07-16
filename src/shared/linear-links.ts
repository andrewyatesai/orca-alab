import { requireOrcaDispatch } from './orca-dispatch-seam'

export function buildLinearTeamUrl(args: {
  organizationUrlKey?: string | null
  teamKey?: string | null
}): string | null {
  const organizationUrlKey = args.organizationUrlKey?.trim()
  const teamKey = args.teamKey?.trim()
  if (!organizationUrlKey || !teamKey) {
    return null
  }
  return `https://linear.app/${encodeURIComponent(organizationUrlKey)}/team/${encodeURIComponent(teamKey)}/all`
}

export function buildLinearPersonalApiKeySettingsUrl(organizationUrlKey?: string | null): string {
  const trimmed = organizationUrlKey?.trim()
  return trimmed
    ? `https://linear.app/${encodeURIComponent(trimmed)}/settings/account/security`
    : 'https://linear.app/settings/account/security'
}

export function buildLinearWorkspaceApiSettingsUrl(organizationUrlKey?: string | null): string {
  const trimmed = organizationUrlKey?.trim()
  return trimmed
    ? `https://linear.app/${encodeURIComponent(trimmed)}/settings/api`
    : 'https://linear.app/settings/api'
}

export function getLinearOrganizationUrlKeyFromIssueUrl(issueUrl?: string | null): string | null {
  if (!issueUrl) {
    return null
  }
  try {
    const parsed = new URL(issueUrl)
    if (parsed.hostname !== 'linear.app') {
      return null
    }
    return parsed.pathname.split('/').find(Boolean) ?? null
  } catch {
    return null
  }
}

export type ParsedLinearIssueInput = {
  identifier: string
  organizationUrlKey?: string
}

// Parse a Linear issue identifier ("ENG-123") or issue URL into its identifier +
// org key. Single-sourced in the Rust core (orca_core::linear_links); this runs
// on main + the CLI (both bind the napi dispatch seam at bootstrap), so it uses
// requireOrcaDispatch. Rust mirrors JS `new URL`/decodeURIComponent/trim/toUpperCase
// via parse_absolute_url + try_decode_uri_component + trim_js.
export function parseLinearIssueInput(input: string): ParsedLinearIssueInput | null {
  return requireOrcaDispatch(
    'linear-links',
    'parseLinearIssueInput',
    input
  ) as ParsedLinearIssueInput | null
}
