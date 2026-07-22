/**
 * Nushell classification + dialect vocabulary (#8928). Detection and quoting
 * only — spawn logic stays in the platform launch modules. Every nu branch in
 * the codebase must call `isNushellExecutableName` instead of re-implementing
 * basename matching.
 */

/** 'nu' / 'nu.exe' / versioned 'nu-0.104' basenames, path or bare, either slash. */
export function isNushellExecutableName(shellPathOrName: string): boolean {
  const basename = shellPathOrName.split(/[\\/]/).pop() ?? ''
  const name = basename.toLowerCase().replace(/\.exe$/, '')
  return name === 'nu' || /^nu-[0-9][0-9a-z.]*$/.test(name)
}

/** Settings/menu sentinel for the Windows shell picker, mirrors WINDOWS_GIT_BASH_SHELL. */
export const WINDOWS_NUSHELL_SHELL = 'nushell'

/** nu double-quoted literal: escapes \ and " ; $ is NOT interpolated in plain "…". */
export function quoteNuDoubleQuoted(value: string): string {
  return `"${value.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`
}

/** `source "<path>"` payload for `-e`. */
export function buildNuSourceCommand(integrationFilePath: string): string {
  return `source ${quoteNuDoubleQuoted(integrationFilePath)}`
}

// Why: floor where every integration-file construct exists ($env.FOO?, try,
// def --env, shell_integration record, char esep); older nu spawns plain -l.
export const NUSHELL_INTEGRATION_MIN_VERSION = '0.96.0'
