import { isNushellExecutableName } from '../../shared/nushell-shell'
import { shellEscape } from './ssh-connection-utils'

const COMMAND_ONLY_SHELLS = new Set(['sh', 'dash', 'csh', 'tcsh'])

// The nu form embeds `shell` in '…' and `command` in "^sh -c '…'"; any quote or
// backslash byte would escape those nesting layers.
const NU_UNSAFE_CHARS = /['"\\]/

/** Build a command using the startup mode supported by the configured login shell. */
export function buildSshLoginShellCommand(shell: string, command: string): string {
  const shellName = shell.split('/').at(-1)
  if (shellName && isNushellExecutableName(shellName)) {
    // Why (#7715): nu rejects combined -lc and cannot run a bare quoted path (needs the ^ caret);
    // the probe body is delegated to sh so nu's login-config PATH (ENV_CONVERSIONS export) applies.
    if (!NU_UNSAFE_CHARS.test(shell) && !NU_UNSAFE_CHARS.test(command)) {
      return `^'${shell}' -l -c "^sh -c '${command}'"`
    }
    // Why: quote-bearing text has no safe nu single-quoted spelling; degrade to the POSIX form
    // (still nu-parseable when the command itself is quote-free, and correct for POSIX shells).
    return `/bin/sh -c ${shellEscape(command)}`
  }
  // Why: csh/tcsh reject combined -lc, while sh/dash do not need login mode here.
  const mode = shellName && COMMAND_ONLY_SHELLS.has(shellName) ? '-c' : '-lc'
  return `${shellEscape(shell)} ${mode} ${shellEscape(command)}`
}
