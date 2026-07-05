# Staging Observability — Fork Telemetry, Feedback, and the Reliability Event Family

Closes audit findings G0-0 … G0-4 (see
`staging-launch-audit-2026-07-04.md` §2.9). This is the operator +
integrator reference for how a staging build produces fork-owned field
data and why it can no longer leak to the public vendor.

## 1. Telemetry keying (G0-0 / G0-3)

Telemetry transmits only when BOTH compile-time constants are injected at
the electron-vite build step (`electron.vite.config.ts` `define` block):

| Env var at build time | Value | Effect |
| --- | --- | --- |
| `ORCA_POSTHOG_WRITE_KEY` | fork PostHog project key (`phc_…`) | enables transport |
| `ORCA_BUILD_IDENTITY` | `rc` (staging) or `stable` | tags `orca_channel`, unlocks `IS_OFFICIAL_BUILD` |

Unset → literal `null` is compiled in and every `track()` is a no-op
(compile-out). **That no-key behavior is intentional and preserved** for
contributor/dev builds. What is no longer possible is *accidentally*
shipping a dark staging artifact:

### The staging preflight gate

`config/scripts/verify-telemetry-constants.mjs --preflight` runs first in
both the `build:desktop` and `build:release` script legs (package.json).
Logic lives in `config/scripts/telemetry-staging-preflight.mjs`:

- **Staging detection:** package.json version contains `-fork.` OR
  `ORCA_STAGING=1`.
- **Non-staging build:** passes with no requirements (upstream-style
  compile-out preserved).
- **Staging build without `ORCA_POSTHOG_WRITE_KEY`:** **fails the build
  loudly**, unless `ORCA_ALLOW_NO_TELEMETRY=1` explicitly opts into a
  "dark staging" artifact (logged as a WARNING).
- **Staging build with a key:** the key must match `phc_[A-Za-z0-9_-]+`,
  `ORCA_BUILD_IDENTITY` must be `rc`/`stable`, and — once the fork PostHog
  project exists — set `ORCA_FORK_POSTHOG_KEY_PREFIX` in the release runner
  so any foreign key (e.g. the extractable **public vendor key, which must
  never be used** — G0-3) fails the build instead of polluting the vendor's
  analytics under continuing public install_ids.

The existing post-pack mode (`node
config/scripts/verify-telemetry-constants.mjs <dist>`) is unchanged: it
greps the packed `app.asar` for the substituted constants and remains the
release-side assertion once artifacts exist.

**No real fork key exists yet.** Provisioning the PostHog project, storing
the key as a runner secret, and exporting `ORCA_FORK_POSTHOG_KEY_PREFIX`
are release-operations tasks; the mechanism above is ready for them.

## 2. Feedback / crash-report egress (G0-2)

`src/main/ipc/feedback.ts` no longer contains any vendor URL. The endpoint
comes from the `ORCA_FEEDBACK_ENDPOINT` compile-time constant; resolution
mirrors `ORCA_DIAGNOSTICS_TOKEN_URL`:

- **Official builds** (`ORCA_BUILD_IDENTITY` injected): pinned to the build
  constant; runtime env cannot redirect.
- **Dev builds:** `ORCA_FEEDBACK_ENDPOINT` env var may point at a scratch
  server.
- **Nothing configured:** `submitFeedback` returns the typed
  `{ ok: false, status: null, error: 'endpoint-not-configured' }`
  (`FEEDBACK_ENDPOINT_NOT_CONFIGURED`) without touching the network. The
  existing renderer surfaces (`SidebarFeedbackDialog`, crash-report dialog)
  already render any `ok: false` as a graceful submission failure. There is
  deliberately **no fallback host** — the old
  `www.onorca.dev`/`api.onorca.dev` fallback chain is gone.

**Build wiring still required (out of this workstream's territory):**
`electron.vite.config.ts` must add `ORCA_FEEDBACK_ENDPOINT` to the main
`define` block (same fold-to-`null` pattern as `ORCA_POSTHOG_WRITE_KEY`;
the module-local `declare const` in feedback.ts is already in place), and
`src/types/build-constants.d.ts` may adopt the declaration if it moves out
of feedback.ts. Until a fork feedback endpoint exists, staging builds fail
closed — users see a submission error and no bytes leave the machine.

`PRIVACY_URL` (`src/renderer/src/lib/telemetry.ts`) now points at the fork
privacy statement (`docs/reference/privacy-staging.md` on the fork repo),
so the consent surfaces (FirstLaunchBanner, PrivacyPane) reference the
fork as data recipient, not the public vendor.

## 3. Fork reliability event family (G0-1 / G0-4)

Schemas: `src/shared/fork-reliability-telemetry.ts`, registered in
`eventSchemas` (`src/shared/telemetry-events.ts`). All payloads are closed
enums — no free-form strings can carry paths, PII, or terminal content.

| Event | Props | Emitted from |
| --- | --- | --- |
| `terminal_gpu_downgrade` | `from: worker\|gpu`, `to: in_process\|cpu`, `reason: worker_init_failed\|gpu_init_failed\|gpu_init_timeout` | **wired** — `aterm-strategy-select.ts` (both fallback warn sites) |
| `renderer_process_gone` | `reason:` snake_cased Electron reason (`clean_exit`…`integrity_failure`, `unknown`) | **wired** — `createMainWindow.ts` `render-process-gone` handler (window-close teardown noise excluded) |
| `daemon_launch_failed` | `error_class: binary_missing\|not_executable\|spawn_failed\|handshake_timeout\|socket_error\|unsupported_platform\|unknown` | **helper exported, hook pending** (below) |
| `daemon_degraded_fallback` | `reason: launch_failed\|preserved_unhealthy\|socket_lost\|unknown` | **helper exported, hook pending** (below) |

Emit helpers: `src/main/telemetry/fork-reliability-events.ts`
(`trackDaemonLaunchFailed`, `trackDaemonDegradedFallback`,
`trackRendererProcessGone`, plus `classifyDaemonLaunchError` which buckets
an unknown error into `error_class` — only the bucket crosses the wire).

### Exact daemon hook points (owned by the failure-visibility workstream)

Do not wire these from other workstreams; they are documented here so the
one-line calls land with the daemon-failure-surfacing changes:

1. **Total daemon launch failure** — `src/main/index.ts`, the
   `onDaemonError` callback (currently `console.error`-only, ~line 632;
   reached via `src/main/window/first-window-startup-services.ts`):

   ```ts
   trackDaemonLaunchFailed(classifyDaemonLaunchError(error))
   trackDaemonDegradedFallback('launch_failed')
   ```

2. **Preserved-daemon degraded mode** — `src/main/daemon/daemon-init.ts`,
   the `launchMode === 'degraded-new-pty-fallback'` branch that constructs
   `DegradedDaemonPtyProvider` (~line 630):

   ```ts
   trackDaemonDegradedFallback('preserved_unhealthy')
   ```

3. **Socket loss after healthy start** (optional, when that surfacing
   lands): `trackDaemonDegradedFallback('socket_lost')` wherever the
   daemon-adapter marks the socket down.

### Renderer spoofing note

`daemon_launch_failed`, `daemon_degraded_fallback`, and
`renderer_process_gone` are main-emitted. Recommended hardening (one line,
in `src/main/ipc/telemetry.ts`, outside this workstream's territory): add
the three names to `MAIN_OWNED_TELEMETRY_EVENTS` so renderer IPC cannot
synthesize them. `terminal_gpu_downgrade` must stay renderer-emittable.

## 4. Consent pipeline (unchanged, verified sound by the audit)

The reliability events flow through the existing gates: burst caps
(30/min/event, 1000/session), live consent resolve on every call
(`DO_NOT_TRACK` / `ORCA_TELEMETRY_DISABLED` / CI / user opt-out), strict
Zod validation, `disableGeoip`, no person profiles. Nothing in this
workstream bypasses them.
