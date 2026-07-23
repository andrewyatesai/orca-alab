/**
 * Result vocabulary for the `terminal:validateShellPath` IPC (#7467): inline
 * validation of an explicit custom shell path entered in Settings. Shared so
 * preload/renderer type the same shape the main-process validator produces.
 */
export type ShellPathValidationFailureReason =
  | 'not-absolute'
  | 'not-found'
  | 'is-directory'
  | 'not-executable'

export type ShellPathValidation =
  | { ok: true; resolvedPath: string }
  | {
      ok: false
      reason: ShellPathValidationFailureReason
      /** For a Store App Execution Alias stub: its recoverable package target. */
      resolvedPath?: string
    }
