import type {
  ProviderRateLimits,
  RateLimitRuntimeTarget
} from '../../../../shared/rate-limit-types'
import { isCodexApiKeyRateLimitError, isCodexAuthError } from '../../../../shared/codex-auth-errors'

type AccountRuntime = {
  runtime: 'host' | 'wsl'
  wslDistro?: string | null
}

export function codexRateLimitTargetMatchesAccountRuntime(
  target: RateLimitRuntimeTarget,
  runtime: AccountRuntime
): boolean {
  if (target.runtime !== runtime.runtime) {
    return false
  }
  if (runtime.runtime === 'host') {
    return true
  }
  return !runtime.wslDistro || target.wslDistro === runtime.wslDistro
}

export function getCodexAccountAuthWarning(args: {
  limits: ProviderRateLimits | null
  target: RateLimitRuntimeTarget
  runtime: AccountRuntime
  activeAccountId: string | null
  accountId: string | null
}): string | null {
  if (args.accountId !== args.activeAccountId) {
    return null
  }
  if (!codexRateLimitTargetMatchesAccountRuntime(args.target, args.runtime)) {
    return null
  }
  if (args.limits?.status !== 'error' || !isCodexAuthError(args.limits.error)) {
    return null
  }
  // Why: an API-key-only provider fails the ChatGPT-specific rateLimits/read (#9313);
  // that's a valid sign-in, so don't prompt a re-authentication that can't fix it.
  if (isCodexApiKeyRateLimitError(args.limits.error)) {
    return null
  }
  return args.limits.error?.trim() || 'Codex reported that this sign-in needs re-authentication.'
}
