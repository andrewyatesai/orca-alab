// Main-process network-proxy normalizers, driven by the Rust orca-net core via
// napi (the shared TS impl was deleted). One source of truth with the
// parity-proven Rust port. The env-precedence getters stay pure TS here — main
// is their only consumer and they are not in the parity oracle — composing the
// napi-backed normalizers.
import { requireRustGitBinding } from './daemon/rust-git-addon'
import type { NetworkProxySettings, ProxyUrlValidationResult } from '../shared/network-proxy'

function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(
    requireRustGitBinding().orcaDispatch('network-proxy', fn, JSON.stringify(input ?? null))
  )
}

export function normalizeProxyUrl(value: unknown): ProxyUrlValidationResult {
  return dispatch('normalizeProxyUrl', value) as ProxyUrlValidationResult
}

export function normalizeProxyBypassRules(value: unknown): string {
  return dispatch('normalizeProxyBypassRules', value) as string
}

export function buildConfiguredProxyEnv(
  settings: NetworkProxySettings | null | undefined
): Record<string, string> {
  return dispatch('buildConfiguredProxyEnv', settings ?? null) as Record<string, string>
}

const PROXY_ENV_KEYS = [
  'HTTPS_PROXY',
  'https_proxy',
  'ALL_PROXY',
  'all_proxy',
  'HTTP_PROXY',
  'http_proxy'
] as const
const NO_PROXY_ENV_KEYS = ['NO_PROXY', 'no_proxy'] as const

export function getProxyUrlFromEnvironment(
  env: Record<string, string | undefined>
): ProxyUrlValidationResult {
  for (const key of PROXY_ENV_KEYS) {
    if (env[key]) {
      return normalizeProxyUrl(env[key])
    }
  }
  return { ok: true, value: '' }
}

export function getProxyBypassRulesFromEnvironment(
  env: Record<string, string | undefined>
): string {
  for (const key of NO_PROXY_ENV_KEYS) {
    if (env[key]) {
      return normalizeProxyBypassRules(env[key])
    }
  }
  return ''
}
