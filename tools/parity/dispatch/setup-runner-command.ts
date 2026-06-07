// TS dispatch for the setup-runner-command parity module: maps the shared
// vector function names to the real `src/shared/setup-runner-command.ts`
// exports so the harness compares the live TS reference against the Rust port.

import {
  buildSetupRunnerCommand,
  type SetupRunnerCommandPlatform
} from '../../../src/shared/setup-runner-command'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'buildSetupRunnerCommand': {
      const { runnerScriptPath, platform } = input as {
        runnerScriptPath: string
        platform: SetupRunnerCommandPlatform
      }
      return buildSetupRunnerCommand(runnerScriptPath, platform)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
