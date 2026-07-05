# Orca Fork — Staging Privacy Statement

**Audience:** the staging cohort of this Orca fork (the builds versioned
`x.y.z-fork.N`, distributed from `andrewyatesai/orc`).
**Effective:** 2026-07-04. This document is the target of the in-app
"privacy" links (Settings → Privacy, and the first-launch telemetry notice).

This is an honest, staging-specific statement. It intentionally does NOT
describe the public Orca product's (`stablyai/orca` / onorca.dev) data
practices — the fork's data goes to the fork team, not to the public vendor.

## Who receives the data

The fork team operating `andrewyatesai/orc`. Nothing is sent to the public
Orca vendor: the fork's builds carry their own PostHog project key and their
own feedback endpoint, and builds without those configured **fail closed**
(they transmit nothing and feedback submission reports a configuration
error rather than falling back to any vendor host).

## What is collected (telemetry)

Only when a build was produced with a fork PostHog write key AND your
consent state is enabled:

- **Common properties on every event:** app version, OS platform / arch /
  release string, an anonymous random `install_id` (UUID), a per-launch
  `session_id` (UUID), and the release channel (`rc`).
- **Event payloads:** closed-enum values validated against strict schemas
  (`src/shared/telemetry-events.ts`). Free-form strings are capped and never
  carry terminal contents, file paths, repository names, or code.
- **Staging reliability events** (why this cohort exists — see
  `staging-observability.md`): `daemon_launch_failed`,
  `daemon_degraded_fallback`, `terminal_gpu_downgrade`,
  `renderer_process_gone`. All payloads are enums (failure class buckets),
  never raw error text.
- **Product events** inherited from upstream Orca (app opened, onboarding
  steps, feature usage, agent start/error classes).

**Not collected:** terminal output or scrollback, prompts or agent
conversations, file contents, file paths, repository names or URLs, git
identity, IP-based geolocation (GeoIP enrichment is disabled), or any
PostHog "person profile".

## Transport

Events go to the fork's own PostHog project (PostHog US cloud,
`us.i.posthog.com`). The fork never uses the public Orca PostHog project
key; the build gate (`config/scripts/verify-telemetry-constants.mjs
--preflight`) refuses to build a staging artifact with a key that does not
match the pinned fork key prefix.

## Consent and defaults — stated honestly

- Staging builds keep upstream's **opt-out** model: telemetry defaults to
  **on** for the staging cohort. This is a deliberate choice for an internal
  staging population whose entire purpose is producing reliability data;
  installing a `-fork.N` staging build is treated as joining that cohort.
- You can turn it off at any time: **Settings → Privacy**, or the
  first-launch notice's "Turn off" action.
- Environment kill switches are honored before any stored preference:
  `DO_NOT_TRACK=1`, `ORCA_TELEMETRY_DISABLED=1`, and CI environments never
  transmit.
- No event transmits before the first-launch notice is resolved for
  pre-existing installs.

## Feedback and crash reports

Feedback and crash reports are sent **only when you explicitly submit
them**, and only to the fork-configured endpoint (`ORCA_FEEDBACK_ENDPOINT`
build constant). If no endpoint is configured, submission fails with an
error and nothing leaves your machine. Crash reports can include a
diagnostic bundle (breadcrumbs/spans); the submit dialog lets you exclude
it, and you may submit anonymously (GitHub identity stripped in the main
process before sending).

## Retention and access

Staging telemetry is used to fix fork reliability issues (daemon failures,
render downgrades, renderer crashes) and is accessible to the fork team
only. Raise deletion requests (by `install_id`, visible to nobody but you
unless you share it) with the fork team via the feedback channel or
andrewyates.m2@pm.me.
