import { getActiveMultiplexer } from './ssh'

export type RemoteWindowsTerminalCapabilities = {
  wslAvailable: boolean
  wslDistros: string[]
  pwshAvailable: boolean
  gitBashAvailable: boolean
  nushellAvailable: boolean
  hostPlatform: NodeJS.Platform | null
}

const EMPTY_REMOTE_WINDOWS_TERMINAL_CAPABILITIES: RemoteWindowsTerminalCapabilities = {
  wslAvailable: false,
  wslDistros: [],
  pwshAvailable: false,
  gitBashAvailable: false,
  nushellAvailable: false,
  hostPlatform: null
}

export async function detectRemoteWindowsTerminalCapabilities(args: {
  connectionId: string
}): Promise<RemoteWindowsTerminalCapabilities> {
  const mux = getActiveMultiplexer(args.connectionId)
  if (!mux || mux.isDisposed()) {
    return EMPTY_REMOTE_WINDOWS_TERMINAL_CAPABILITIES
  }
  const result = (await mux.request('preflight.detectWindowsTerminalCapabilities', {})) as
    | RemoteWindowsTerminalCapabilities
    | undefined
  if (!result) {
    return EMPTY_REMOTE_WINDOWS_TERMINAL_CAPABILITIES
  }
  // Why: an older deployed relay omits nushellAvailable; keep the declared boolean honest.
  return { ...result, nushellAvailable: result.nushellAvailable ?? false }
}
