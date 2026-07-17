import type { TuiAgent } from '../../shared/types'
import type { ShellReadyState, TerminalSnapshot } from './types'

export type DaemonCreateOrAttachResult = {
  isNew: boolean
  snapshot: TerminalSnapshot | null
  pid: number | null
  shellState: ShellReadyState
  historySeeded?: boolean
  launchAgent?: TuiAgent
  wslDistro?: string
}

export function getDaemonSessionResultMetadata(session: {
  launchAgent: TuiAgent | null
  historySeeded: boolean | undefined
  wslDistro: string | null
}): Pick<DaemonCreateOrAttachResult, 'launchAgent' | 'historySeeded' | 'wslDistro'> {
  return {
    ...(session.launchAgent ? { launchAgent: session.launchAgent } : {}),
    ...(session.historySeeded !== undefined ? { historySeeded: session.historySeeded } : {}),
    ...(session.wslDistro ? { wslDistro: session.wslDistro } : {})
  }
}
