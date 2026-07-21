import { isExpectedNodeSqliteWarning } from './vitest-warning-policy'

const FILTER_INSTALLED = Symbol.for('orca.vitest.warningFilterInstalled')
const processState = process as NodeJS.Process & Record<PropertyKey, unknown>

if (processState[FILTER_INSTALLED] !== true) {
  const emitWarning = process.emitWarning.bind(process)

  process.emitWarning = ((
    warning: string | Error,
    typeOrOptions?: string | { type?: string },
    ...rest: unknown[]
  ) => {
    if (isExpectedNodeSqliteWarning(warning, typeOrOptions)) {
      return
    }
    Reflect.apply(emitWarning, process, [warning, typeOrOptions, ...rest])
  }) as typeof process.emitWarning

  processState[FILTER_INSTALLED] = true
}
