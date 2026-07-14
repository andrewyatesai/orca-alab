import { session } from 'electron'
import {
  getProxyBypassRulesFromEnvironment,
  getProxyUrlFromEnvironment,
  normalizeProxyBypassRules,
  normalizeProxyUrl
} from '../rust-network-proxy'
import type { NetworkProxySettings } from '../../shared/network-proxy'

type ProxySession = {
  resolveProxy(url: string): Promise<string>
  setProxy(config: {
    mode?: 'system' | 'fixed_servers'
    proxyRules?: string
    proxyBypassRules?: string
  }): Promise<void>
  closeAllConnections?: () => Promise<void>
}

export type ProxyApplyResult =
  | { source: 'settings'; proxyRules: string; proxyBypassRules?: string }
  | { source: 'env'; proxyRules: string; proxyBypassRules?: string }
  | { source: 'system' | 'none' | 'invalid-settings' | 'invalid-env' }

const PROXY_PROBE_URL = 'https://api.anthropic.com/'

let lastAppliedProxyConfig: Extract<ProxyApplyResult, { source: 'settings' | 'env' }> | null = null

// Why: startup applies the persisted/settings proxy asynchronously — it can
// block on WPAD/PAC discovery for hundreds of ms on enterprise auto-detect
// networks — so app-owned fetchers await this gate before their first request.
// Without it, a request could race ahead of an explicit settings proxy and go
// out on the wrong route. Defaults resolved so paths that never run startup
// application (tests, headless serve before it settles) never block.
let proxyReady: Promise<void> = Promise.resolve()

/**
 * Arm the one-time startup proxy-readiness gate and return a resolver to call
 * once the initial `applyElectronProxySettings` settles (success or failure).
 * Call synchronously before kicking off that initial application.
 */
export function beginInitialProxyApplication(): () => void {
  let resolve!: () => void
  proxyReady = new Promise<void>((r) => {
    resolve = r
  })
  return resolve
}

/**
 * Await the one-time startup proxy application before the first app-owned
 * network request. Resolves immediately when startup application never ran.
 */
export function whenProxyReady(): Promise<void> {
  return proxyReady
}

async function setSessionProxy(
  proxySession: ProxySession,
  config: Parameters<ProxySession['setProxy']>[0]
): Promise<void> {
  await proxySession.setProxy(config)
  await proxySession.closeAllConnections?.()
}

export function resetProxyApplicationForTests(): void {
  lastAppliedProxyConfig = null
  proxyReady = Promise.resolve()
}

/**
 * Ensure the network proxy is applied before an app-owned request.
 *
 * Why: waits for the one-time startup proxy application (which may block on
 * WPAD/PAC) before probing/bridging env proxy, so an explicit settings proxy
 * is applied before this request selects its route. Delegates to the env
 * bridge, which memoizes the applied config so this stays a cheap no-op once
 * the session proxy is settled.
 */
export async function ensureElectronProxyForRequest(
  options: {
    proxySession?: ProxySession
    env?: Record<string, string | undefined>
    force?: boolean
    probeUrl?: string
  } = {}
): Promise<ProxyApplyResult> {
  await proxyReady
  return ensureElectronProxyFromEnvironment(options)
}

export async function ensureElectronProxyFromEnvironment(
  options: {
    proxySession?: ProxySession
    env?: Record<string, string | undefined>
    force?: boolean
    probeUrl?: string
  } = {}
): Promise<ProxyApplyResult> {
  if (!options.force && lastAppliedProxyConfig !== null) {
    return lastAppliedProxyConfig
  }

  const proxySession = options.proxySession ?? session.defaultSession
  const resolved = await proxySession.resolveProxy(options.probeUrl ?? PROXY_PROBE_URL)
  if (resolved !== 'DIRECT') {
    return { source: 'system' }
  }

  const proxy = getProxyUrlFromEnvironment(options.env ?? process.env)
  if (!proxy.ok) {
    return { source: 'invalid-env' }
  }
  if (!proxy.value) {
    return { source: 'none' }
  }

  const bypassRules = getProxyBypassRulesFromEnvironment(options.env ?? process.env)
  await setSessionProxy(proxySession, {
    mode: 'fixed_servers',
    proxyRules: proxy.value,
    ...(bypassRules ? { proxyBypassRules: bypassRules } : {})
  })
  lastAppliedProxyConfig = {
    source: 'env',
    proxyRules: proxy.value,
    ...(bypassRules ? { proxyBypassRules: bypassRules } : {})
  }
  return lastAppliedProxyConfig
}

export async function applyElectronProxySettings(
  settings: NetworkProxySettings,
  options: {
    proxySession?: ProxySession
    env?: Record<string, string | undefined>
    probeUrl?: string
  } = {}
): Promise<ProxyApplyResult> {
  const proxySession = options.proxySession ?? session.defaultSession
  const proxy = normalizeProxyUrl(settings.httpProxyUrl)
  if (!proxy.ok) {
    return ensureElectronProxyFromEnvironment({
      proxySession,
      env: options.env,
      force: lastAppliedProxyConfig !== null,
      probeUrl: options.probeUrl
    }).then((result) => (result.source === 'none' ? { source: 'invalid-settings' } : result))
  }

  if (proxy.value) {
    const bypassRules = normalizeProxyBypassRules(settings.httpProxyBypassRules)
    await setSessionProxy(proxySession, {
      mode: 'fixed_servers',
      proxyRules: proxy.value,
      ...(bypassRules ? { proxyBypassRules: bypassRules } : {})
    })
    lastAppliedProxyConfig = {
      source: 'settings',
      proxyRules: proxy.value,
      ...(bypassRules ? { proxyBypassRules: bypassRules } : {})
    }
    return lastAppliedProxyConfig
  }

  if (lastAppliedProxyConfig !== null) {
    await setSessionProxy(proxySession, { mode: 'system' })
    lastAppliedProxyConfig = null
  }
  return ensureElectronProxyFromEnvironment({
    proxySession,
    env: options.env,
    force: true,
    probeUrl: options.probeUrl
  })
}
