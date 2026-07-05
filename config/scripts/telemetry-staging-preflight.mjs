// Staging telemetry preflight — the build-time half of the G0-0/G0-3 gate.
//
// A staging build (a `-fork.N` version, or any build with ORCA_STAGING=1)
// exists to produce field observability. Building one without a PostHog
// write key compiles telemetry out to a silent no-op and the cohort ships
// blind — so the preflight FAILS LOUDLY unless ORCA_ALLOW_NO_TELEMETRY=1
// explicitly opts into "dark staging". Non-staging builds are untouched:
// contributor / dev / upstream-style builds keep the no-key compile-out.
//
// The key must be a FORK-provisioned PostHog project key. The public
// vendor's key is trivially extractable from public binaries, but pasting
// it here would silently pollute the vendor's analytics under continuing
// public install_ids (G0-3). Once the fork project exists, pin its key
// prefix via ORCA_FORK_POSTHOG_KEY_PREFIX (e.g. in the release runner env)
// so a foreign key fails the build instead of shipping.
//
// Pure module (no process.exit, no top-level execution) so the vitest suite
// can exercise both branches directly; verify-telemetry-constants.mjs owns
// the CLI wiring. Cross-platform: plain Node, no shell dependencies.

const POSTHOG_KEY_RE = /^phc_[A-Za-z0-9_-]+$/

/**
 * @param {object} input
 * @param {string} input.version package.json version of the build
 * @param {Record<string, string | undefined>} input.env process env
 * @returns {{ ok: boolean, staging: boolean, dark: boolean, messages: string[], errors: string[] }}
 */
export function runStagingTelemetryPreflight({ version, env }) {
  const messages = []
  const errors = []

  const staging = version.includes('-fork.') || env.ORCA_STAGING === '1'
  if (!staging) {
    messages.push(
      `Not a staging build (version "${version}" has no -fork. suffix and ORCA_STAGING is unset) — ` +
        'telemetry constants are optional; compile-out (no-key) behavior applies.'
    )
    return { ok: true, staging: false, dark: false, messages, errors }
  }

  const writeKey = env.ORCA_POSTHOG_WRITE_KEY
  const identity = env.ORCA_BUILD_IDENTITY

  if (!writeKey) {
    if (env.ORCA_ALLOW_NO_TELEMETRY === '1') {
      messages.push(
        'WARNING: DARK STAGING BUILD. ORCA_POSTHOG_WRITE_KEY is unset and ' +
          'ORCA_ALLOW_NO_TELEMETRY=1 opted into building anyway. This artifact ' +
          'transmits NO telemetry — the staging cohort will produce zero field ' +
          'data. Ship it only if that is a deliberate decision.'
      )
      return { ok: true, staging: true, dark: true, messages, errors }
    }
    errors.push(
      'staging build requires telemetry constants: ORCA_POSTHOG_WRITE_KEY is unset. ' +
        'A staging artifact built without it compiles telemetry out and ships silent. ' +
        'Export the FORK PostHog project key (never the public vendor key) plus ' +
        'ORCA_BUILD_IDENTITY=rc before the build, or set ORCA_ALLOW_NO_TELEMETRY=1 ' +
        'to explicitly build a dark (no-telemetry) staging artifact.'
    )
    return { ok: false, staging: true, dark: false, messages, errors }
  }

  if (!POSTHOG_KEY_RE.test(writeKey)) {
    errors.push(
      'ORCA_POSTHOG_WRITE_KEY is set but is not a valid PostHog project key ' +
        '(expected phc_ followed by URL-safe base64).'
    )
  }

  if (identity !== 'rc' && identity !== 'stable') {
    errors.push(
      `ORCA_BUILD_IDENTITY must be "rc" or "stable" when a write key is injected (got ${
        identity === undefined ? 'unset' : JSON.stringify(identity)
      }). The runtime treats one-without-the-other as a pipeline misconfiguration ` +
        'and fails closed to a silent build.'
    )
  }

  const forkPrefix = env.ORCA_FORK_POSTHOG_KEY_PREFIX
  if (forkPrefix && !writeKey.startsWith(forkPrefix)) {
    errors.push(
      'ORCA_POSTHOG_WRITE_KEY does not match ORCA_FORK_POSTHOG_KEY_PREFIX. ' +
        'Refusing to build: a non-fork key would send staging events to a foreign ' +
        '(public vendor) PostHog project under continuing public install_ids.'
    )
  }

  if (errors.length > 0) {
    return { ok: false, staging: true, dark: false, messages, errors }
  }

  messages.push(
    `Staging telemetry preflight OK: ORCA_BUILD_IDENTITY="${identity}", ` +
      `ORCA_POSTHOG_WRITE_KEY="${writeKey.slice(0, 8)}..." (length=${writeKey.length}).`
  )
  return { ok: true, staging: true, dark: false, messages, errors }
}
