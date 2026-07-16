// TS dispatch for the linear-links parity module: maps the shared vector
// function names to the real `src/shared/linear-links.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  buildLinearPersonalApiKeySettingsUrl,
  buildLinearTeamUrl,
  buildLinearWorkspaceApiSettingsUrl,
  getLinearOrganizationUrlKeyFromIssueUrl
} from '../../../src/shared/linear-links'
// parseLinearIssueInput is cut over to the Rust core (napi in main + cli), so it
// drives that binding directly — the vectors' TS-derived goldens pin it, and the
// TS-vs-Rust diff degenerates to napi-vs-binary. The other 4 stay live TS.
import { requireRustGitBinding } from '../../../src/main/daemon/rust-git-addon'

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
    case 'parseLinearIssueInput':
      return JSON.parse(
        requireRustGitBinding().orcaDispatch(
          'linear-links',
          'parseLinearIssueInput',
          JSON.stringify(input ?? null)
        )
      )
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
