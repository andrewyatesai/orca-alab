// TS dispatch for the nested-repo-telemetry parity module: maps the shared
// vector function names to the real `src/shared/nested-repo-telemetry.ts`
// exports so the harness compares the live TS reference against the Rust port.

import {
  buildNestedRepoImportActionTelemetry,
  buildNestedRepoScanTelemetry,
  bucketNestedRepoTelemetryCount,
  capNestedRepoTelemetryCount,
  shouldEmitNestedRepoImportSubmitTelemetry,
  type NestedRepoImportTelemetryAction,
  type NestedRepoTelemetryRuntimeKind,
  type NestedRepoTelemetrySurface
} from '../../../src/shared/nested-repo-telemetry'
import type { NestedRepoScanResult } from '../../../src/shared/types'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'capNestedRepoTelemetryCount':
      // NaN/Infinity persist to JSON as null; cap treats non-finite as 0.
      return capNestedRepoTelemetryCount(input as number)
    case 'bucketNestedRepoTelemetryCount':
      return bucketNestedRepoTelemetryCount(input as number)
    case 'shouldEmitNestedRepoImportSubmitTelemetry': {
      const { attemptId, selectedCount, isBusy } = input as {
        attemptId: string | null
        selectedCount: number
        isBusy?: boolean
      }
      return shouldEmitNestedRepoImportSubmitTelemetry({ attemptId, selectedCount, isBusy })
    }
    case 'buildNestedRepoScanTelemetry': {
      const { attemptId, surface, runtimeKind, scan } = input as {
        attemptId: string
        surface: NestedRepoTelemetrySurface
        runtimeKind: NestedRepoTelemetryRuntimeKind
        scan: NestedRepoScanResult | null
      }
      return buildNestedRepoScanTelemetry({ attemptId, surface, runtimeKind, scan })
    }
    case 'buildNestedRepoImportActionTelemetry': {
      const { attemptId, surface, runtimeKind, action, foundCount, selectedCount } = input as {
        attemptId: string
        surface: NestedRepoTelemetrySurface
        runtimeKind: NestedRepoTelemetryRuntimeKind
        action: NestedRepoImportTelemetryAction
        foundCount: number
        selectedCount: number
      }
      return buildNestedRepoImportActionTelemetry({
        attemptId,
        surface,
        runtimeKind,
        action,
        foundCount,
        selectedCount
      })
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
