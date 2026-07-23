import { useEffect, useRef, useState } from 'react'
import { Input } from '../ui/input'
import { translate } from '@/i18n/i18n'
import type {
  ShellPathValidation,
  ShellPathValidationFailureReason
} from '../../../../shared/terminal-shell-path-validation'

const VALIDATE_DEBOUNCE_MS = 300

function validationErrorMessage(
  reason: ShellPathValidationFailureReason,
  resolvedPath: string | undefined
): string {
  switch (reason) {
    case 'not-absolute':
      return translate(
        'auto.components.settings.TerminalCustomShellPathInput.notAbsolute',
        'Enter an absolute path to the shell executable.'
      )
    case 'not-found':
      return translate(
        'auto.components.settings.TerminalCustomShellPathInput.notFound',
        'No file exists at this path.'
      )
    case 'is-directory':
      return translate(
        'auto.components.settings.TerminalCustomShellPathInput.isDirectory',
        'This path is a directory, not a shell executable.'
      )
    case 'not-executable':
      return resolvedPath
        ? translate(
            'auto.components.settings.TerminalCustomShellPathInput.aliasNotExecutable',
            'This is a Store app alias that terminals cannot launch. Try {{target}}.',
            { target: resolvedPath }
          )
        : translate(
            'auto.components.settings.TerminalCustomShellPathInput.notExecutable',
            'This file is not an executable shell.'
          )
  }
}

type TerminalCustomShellPathInputProps = {
  inputId: string
  /** Persisted custom path ('' when Custom… was just selected with no path yet). */
  value: string
  placeholder: string
  onCommit: (path: string) => void
}

/**
 * Free-text custom shell path field (#7467) shared by the Windows and POSIX
 * shell sections: commits the path verbatim on blur/Enter and shows debounced
 * inline validation from the local terminal host.
 */
export function TerminalCustomShellPathInput({
  inputId,
  value,
  placeholder,
  onCommit
}: TerminalCustomShellPathInputProps): React.JSX.Element {
  const [draft, setDraft] = useState(value)
  const [validation, setValidation] = useState<ShellPathValidation | null>(null)
  const validationSeq = useRef(0)

  // Why: an external settings change (e.g. picking a built-in then Custom… again) must reseed the draft.
  useEffect(() => {
    setDraft(value)
  }, [value])

  useEffect(() => {
    const seq = ++validationSeq.current
    if (!draft.trim()) {
      setValidation(null)
      return
    }
    const timer = setTimeout(() => {
      const validate = window.api.terminalShell?.validatePath
      if (!validate) {
        return
      }
      validate(draft)
        .then((result) => {
          // Why: null means the host cannot validate (web client) — show nothing rather than a false error.
          if (validationSeq.current === seq) {
            setValidation(result)
          }
        })
        .catch(() => {})
    }, VALIDATE_DEBOUNCE_MS)
    return () => clearTimeout(timer)
  }, [draft])

  const commit = (): void => {
    const trimmed = draft.trim()
    if (trimmed && trimmed !== value) {
      onCommit(trimmed)
    }
  }

  return (
    <div className="space-y-1">
      <Input
        id={inputId}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.currentTarget.blur()
          }
        }}
        placeholder={placeholder}
        autoCapitalize="none"
        autoCorrect="off"
        autoComplete="off"
        spellCheck={false}
        aria-invalid={validation && !validation.ok ? true : undefined}
        className="font-mono text-xs"
      />
      {validation && !validation.ok ? (
        <p className="text-xs text-destructive">
          {validationErrorMessage(validation.reason, validation.resolvedPath)}
        </p>
      ) : null}
    </div>
  )
}
