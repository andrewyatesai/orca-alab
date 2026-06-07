// TS dispatch for the linear-links parity module: maps the shared vector
// function names to the real `src/shared/linear-links.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  buildLinearPersonalApiKeySettingsUrl,
  buildLinearTeamUrl,
  buildLinearWorkspaceApiSettingsUrl,
  getLinearOrganizationUrlKeyFromIssueUrl
} from '../../../src/shared/linear-links'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'buildLinearTeamUrl': {
      const { organizationUrlKey, teamKey } = input as {
        organizationUrlKey?: string | null
        teamKey?: string | null
      }
      return buildLinearTeamUrl({ organizationUrlKey, teamKey })
    }
    case 'buildLinearPersonalApiKeySettingsUrl':
      return buildLinearPersonalApiKeySettingsUrl(input as string | null | undefined)
    case 'buildLinearWorkspaceApiSettingsUrl':
      return buildLinearWorkspaceApiSettingsUrl(input as string | null | undefined)
    case 'getLinearOrganizationUrlKeyFromIssueUrl':
      return getLinearOrganizationUrlKeyFromIssueUrl(input as string | null | undefined)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
