# Daemon staleness & failure-visibility UX

Design record for how Orca surfaces terminal-daemon health to the user. The
code cites this doc from `src/main/daemon/daemon-init.ts`,
`src/main/ipc/pty-management.ts`, and `src/main/window/attach-main-window-services.ts`.

## Phase 1 — Manage Sessions & daemon restart

The Settings → Terminal → Manage Sessions panel exposes a narrow
`pty:management:*` IPC surface (list / kill-one / kill-all / restart) so a
user can recover from a wedged terminal without quitting the app.

### Restart sequence

`restartDaemon()` (daemon-init.ts) is a 7-step sequence; ordering is the
invariant, not an implementation detail:

1. Synthesize `pty:exit` for every active session on the current adapter
   **before any teardown** — the daemon's kill-all path does not fan exits to
   clients, so skipping this black-holes renderer writes. (In degraded mode,
   local-fallback sessions are shut down and current-daemon sessions get
   synthetic exits here too.)
2. Unbind renderer listeners from the outgoing provider (after step 1 so the
   synthetic exits are actually delivered).
3. Kill the current-protocol daemon (`cleanupDaemonForProtocol`).
4. Reuse the existing `DaemonSpawner` (`resetHandle()` + `ensureRunning()`) so
   respawn closures baked into long-lived adapters stay valid.
5. Build a fresh current adapter against the respawned daemon.
6. Swap module state atomically (`replaceDaemonProvider`).
7. Rebind renderer listeners against the new provider.

### Scope rationale

Restart is **current-protocol only**. Legacy-protocol daemons (from a previous
app version) may be hosting long-lived agent sessions; they are preserved by
reference and never respawned — respawning an old protocol from new code would
recreate stale env semantics. `killAll`, by contrast, operates on *sessions*
(the user-facing concept) and fans across current + legacy adapters; the
daemon processes themselves survive.

## Phase 2 — failure visibility (daemon-status registry)

A daemon that silently fails or degrades turns off terminal persistence with
no signal; users discover dead terminals only after a restart. Phase 2 makes
every non-`running` state loud once and quietly persistent after.

### States

The main process keeps a single source of truth in
`src/main/ipc/daemon-status-registry.ts`:

| state                | meaning                                                                    |
| -------------------- | -------------------------------------------------------------------------- |
| `starting`           | init (or an explicit relaunch) is in flight; transient, never surfaced     |
| `running`            | daemon-backed provider installed; fresh terminals are persistent           |
| `degraded-fallback`  | fresh spawns run on the in-process local provider without persistence      |
| `failed`             | daemon launch/restart threw; no daemon provider is installed               |

Each status carries a machine-readable `cause`
(`launch-failed` / `startup-timeout` / `spawn-unhealthy` / `restart-failed`) so
the renderer localizes copy, plus a raw `detail` string (the launch error) for
diagnostics, and `updatedAt`.

Feeders: `initDaemonPtyProvider` (success / degraded launch mode / abort via
startup fail-open / throw) and `restartDaemon` (success / throw). Both flow
through the registry so every caller — the settings Restart button, the toast
Retry, background init — updates the same state.

### Surfacing (renderer)

1. **Sticky toast** on *entering* `degraded-fallback` or `failed` — mirrors the
   "Session restore failed" pattern (App.tsx): `duration: Infinity`,
   dismissible, single stable toast id, with a **Retry / Restart daemon**
   action wired to `daemon:status:relaunch`. Dismissed automatically when the
   state recovers; recovery to `running` shows a success toast.
2. **Low-key persistent indicator** in the status bar
   (`DaemonStatusSegment`, alongside the SSH/update segments) whenever the
   state is not `running`/`starting`. Tooltip carries the localized
   explanation plus the raw `detail`; clicking opens Settings → Terminal
   (Manage Sessions), where the guarded restart/kill flows live.
3. **Manage Sessions banner** (already present for `degraded`) additionally
   distinguishes total failure and shows the `detail` string.

### Relaunch semantics (`daemon:status:relaunch`)

- State `running`: no-op success (a Retry click can race a background recovery;
  never restart a healthy daemon from this path).
- State `starting` with no provider installed: no-op success — an init attempt
  is already in flight and racing a second init would double-spawn daemons;
  the registry transition announces the outcome.
- A daemon provider is installed (degraded case): reuse `restartDaemon()` — the
  Phase 1 sequence already handles fallback-session shutdown and synthetic exits.
- No provider installed (total launch failure): fresh terminals spawned on the
  in-process `LocalPtyProvider`. Those sessions are killed *through the
  still-bound local provider* first (so real `pty:exit` events reach the
  renderer), then `initDaemonPtyProvider()` runs again. Killing first is
  required: the provider swap at the end of init re-routes PTY IPC, and ids the
  daemon provider doesn't know would black-hole writes — the exact failure mode
  this feature exists to prevent. Toast copy warns that retrying closes open
  terminal panes.
- Concurrent relaunch requests coalesce onto one in-flight promise (mirroring
  `restartDaemon`'s coalescer).
- The settings Restart button (`pty:management:restart`) routes through this
  recovery path whenever no provider is installed, so it also works after a
  total launch failure; with a provider installed it calls `restartDaemon()`
  directly (restarting a healthy daemon must actually bounce the process).
  `restartDaemon()` itself rejects its no-provider precondition *before* the
  status hooks, so it never overwrites the registry's launch-failed detail
  with `restart-failed`.

### Non-goals

- No auto-retry loop: a failing daemon binary would flap. Retry is
  user-initiated.
- Windows uses the Node named-pipe daemon; the registry and UX are identical —
  states are provider-level, not transport-level.
- Remote/SSH runtimes: the registry describes the *local* desktop daemon only;
  remote terminal transport health is surfaced by the SSH/runtime segments.
