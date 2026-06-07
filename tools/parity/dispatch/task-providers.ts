// TS dispatch for the task-providers parity module: maps the shared vector
// function names to the real `src/shared/task-providers.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  isTaskProvider,
  normalizeTaskProviderSettings,
  normalizeVisibleTaskProviders,
  resolveVisibleTaskProvider,
  restoreAvailableDefaultTaskProvider,
  type TaskProvider,
  type TaskProviderAvailability
} from '../../../src/shared/task-providers'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'isTaskProvider':
      return isTaskProvider(input)
    case 'normalizeVisibleTaskProviders':
      return normalizeVisibleTaskProviders(input)
    case 'normalizeTaskProviderSettings':
      return normalizeTaskProviderSettings(
        input as { visibleTaskProviders: unknown; defaultTaskSource: unknown }
      )
    case 'restoreAvailableDefaultTaskProvider': {
      const { visibleProviders, availability, preferredProvider } = input as {
        visibleProviders: TaskProvider[]
        availability: TaskProviderAvailability
        preferredProvider: unknown
      }
      return restoreAvailableDefaultTaskProvider(visibleProviders, availability, preferredProvider)
    }
    case 'resolveVisibleTaskProvider': {
      const { preferred, visibleProviders } = input as {
        preferred: TaskProvider | null | undefined
        visibleProviders: TaskProvider[]
      }
      return resolveVisibleTaskProvider(preferred, visibleProviders)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
