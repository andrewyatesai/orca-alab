const NODE_SQLITE_EXPERIMENTAL_WARNING =
  'SQLite is an experimental feature and might change at any time'

type WarningTypeOrOptions = string | { type?: string } | undefined

function warningType(typeOrOptions: WarningTypeOrOptions): string | undefined {
  return typeof typeOrOptions === 'string' ? typeOrOptions : typeOrOptions?.type
}

/**
 * Node 24 marks the built-in SQLite module as experimental even though Orca
 * deliberately exercises it. Match both message and warning class so a new or
 * unrelated experimental warning remains visible to the test runner.
 */
export function isExpectedNodeSqliteWarning(
  warning: string | Error,
  typeOrOptions?: WarningTypeOrOptions
): boolean {
  const message = typeof warning === 'string' ? warning : warning.message
  const type = warning instanceof Error ? warning.name : warningType(typeOrOptions)
  return message === NODE_SQLITE_EXPERIMENTAL_WARNING && type === 'ExperimentalWarning'
}
