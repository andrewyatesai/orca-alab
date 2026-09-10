import type { ConnectionLogLevel, ConnectionLogSink } from './types'

export type ConnectionLogEmitter = (
  level: ConnectionLogLevel,
  message: string,
  detail?: string
) => void

export function createConnectionLogEmitter(
  onLog: ConnectionLogSink | undefined
): ConnectionLogEmitter {
  let logCounter = 0
  return function emitLog(level: ConnectionLogLevel, message: string, detail?: string) {
    if (!onLog) {
      return
    }
    onLog({
      id: `log-${++logCounter}-${Date.now()}`,
      ts: Date.now(),
      level,
      message,
      detail
    })
  }
}

// URL parsing also strips query credentials when an endpoint has no path.
export function redactedEndpoint(ep: string): string {
  try {
    const endpoint = new URL(ep)
    if (endpoint.protocol !== 'ws:' && endpoint.protocol !== 'wss:') {
      return 'unknown'
    }
    return endpoint.host || 'unknown'
  } catch {
    return 'unknown'
  }
}
