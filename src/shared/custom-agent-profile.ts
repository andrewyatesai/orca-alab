import type { CustomAgentProfile } from './types'

/** Quote a single env-var value for the user's interactive shell.
 *  - On POSIX shells (bash/zsh/fish) the launch is wrapped as `KEY='value' cmd…`.
 *    Single-quote everything and escape embedded single quotes via the classic
 *    `'\''` close-reopen trick.
 *  - On Windows we go through PowerShell or CMD; both honor `$env:KEY=...` /
 *    `set "KEY=…"` syntaxes, but the existing startup-command path already
 *    feeds shell-evaluated strings, so we use the cross-platform-safe form
 *    `KEY="value"` with embedded double quotes escaped as `""`. PowerShell
 *    accepts `KEY=value` as a positional env-prefix only via `cmd /c`, so on
 *    Windows callers should set `command` to a full `cmd /c …` string when
 *    they need env vars; the prefix model below stays POSIX-style and we
 *    surface a UI hint for Windows users in the settings pane.
 */
function quoteEnvValuePosix(value: string): string {
  return `'${value.replace(/'/g, `'\\''`)}'`
}

function quoteEnvValueWindows(value: string): string {
  return `"${value.replace(/"/g, '""')}"`
}

/** Build the `KEY1='v1' KEY2='v2' ` shell prefix for a profile's env map.
 *  Returns an empty string when the map is missing or empty so callers can
 *  unconditionally concatenate. */
export function buildEnvShellPrefix(
  env: Record<string, string> | undefined,
  platform: NodeJS.Platform
): string {
  if (!env) {
    return ''
  }
  const entries = Object.entries(env).filter(([key]) => key.length > 0)
  if (entries.length === 0) {
    return ''
  }
  const quote = platform === 'win32' ? quoteEnvValueWindows : quoteEnvValuePosix
  return `${entries.map(([key, value]) => `${key}=${quote(value)}`).join(' ')} `
}

/** Resolve the base launch command for a custom profile: env prefix + user
 *  command. Returns a single shell-evaluated string, e.g.
 *  `ANTHROPIC_BASE_URL='http://localhost:1234' claude`. */
export function resolveCustomAgentBaseCommand(
  profile: CustomAgentProfile,
  platform: NodeJS.Platform
): string {
  const prefix = buildEnvShellPrefix(profile.env, platform)
  return `${prefix}${profile.command}`.trimStart()
}
