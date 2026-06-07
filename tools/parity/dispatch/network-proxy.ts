// TS dispatch for the network-proxy parity module: maps the shared vector
// function names to the real `src/shared/network-proxy.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  buildConfiguredProxyEnv,
  normalizeProxyBypassRules,
  normalizeProxyUrl,
  redactProxyUrl,
  type NetworkProxySettings
} from '../../../src/shared/network-proxy'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'normalizeProxyUrl':
      return normalizeProxyUrl(input)
    case 'normalizeProxyBypassRules':
      return normalizeProxyBypassRules(input)
    case 'buildConfiguredProxyEnv':
      return buildConfiguredProxyEnv(input as NetworkProxySettings | null)
    case 'redactProxyUrl':
      return redactProxyUrl(input as string)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
