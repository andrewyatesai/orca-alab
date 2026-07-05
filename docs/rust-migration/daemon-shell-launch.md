# Daemon shell-launch layer (audit F4 remediation)

Status: landed 2026-07-04 (staging-launch audit F4b/F4c/F4d + G3 protocol split).

## Problem

The Rust daemon (`rust/crates/orca-daemon`) replaced the Node daemon on
macOS/Linux but dropped the shell-launch layer the Node daemon had grown:

- plain sessions spawned `$SHELL` with **no args** (no `-l` → no
  `.zprofile`/`.zlogin` env, so brew shellenv PATH etc. was missing);
- startup commands ran under non-interactive `shell -lc <cmd>` (the shell
  exits with the agent; nvm/mise-provided CLIs miss rc-file PATH);
- no shell-ready barrier: nothing waited for the first prompt before typing
  the startup command, and no pre-ready stdin queue existed;
- the ZDOTDIR / `--rcfile` wrapper layer (OSC 133 marks, attribution PATH
  shims, `CODEX_HOME`/OpenCode-config restoration, the OSC 777 ready marker)
  was never applied.

## Design: client-computed launch config, daemon-run barrier

The wrapper rcfiles already exist in TS and are shared with the local
provider (`src/main/providers/local-pty-shell-ready.ts`, backed by
`src/main/shell-templates.ts`). Porting their generation to Rust would create
a fourth drifting copy (audit F40 already flags three). So the split is:

**Client (Electron main, `src/main/daemon/daemon-shell-launch-config.ts`)**
pre-computes the final launch on POSIX and ships it in `createOrAttach`:

- resolves the shell (`shellOverride` → `env.SHELL` → `$SHELL` → `/bin/zsh`);
- picks the wrapper exactly like the Node daemon's `pty-subprocess.ts` POSIX
  branch: marker wrapper for command sessions (and payload-bearing Codex),
  attribution wrapper for markerless Codex and for plain sessions whose
  launch-mode env must survive rc files, plain `['-l']` otherwise;
- ensures the wrapper files on disk (same `<userData>/shell-ready` root the
  local provider uses) and merges the wrapper env (ZDOTDIR chain,
  `ORCA_SHELL_READY_MARKER`, …) into the payload env;
- seeds `POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD`
  (`seedPowerlevel10kWizardEnv`, same call as both Node spawn seams) so an
  unconfigured p10k user's first-run wizard can't block `.zshrc` — which
  would starve the ready marker and dump the queued startup command into the
  wizard prompt after the barrier timeout;
- applies the macOS `login(1)` TCC-attribution wrap
  (`macos-tcc-login-shell.ts`) so daemon terminals keep their own TCC
  identity, exactly like both Node spawn seams;
- sends the result as `shellOverride` (program) + `shellArgs` (verbatim argv),
  a v1018 wire addition (`types.ts` `CreateOrAttachRequest.shellArgs`).

The pre-compute is gated on the adapter speaking the fork protocol
(`protocolVersion === PROTOCOL_VERSION`). A LEGACY adapter — the live public
Node daemon (v18) attached via `PREVIOUS_DAEMON_PROTOCOL_VERSIONS`, which the
router deliberately keeps routing to across sleep/wake — ignores `shellArgs`
and computes its own args from `shellOverride`, so shipping the wrap there
would spawn `/usr/bin/login` as the "shell" on macOS: an interactive `login:`
password prompt on the first wake respawn. Legacy adapters keep the plain
`opts.shellOverride`/`opts.env` passthrough
(`daemon-pty-adapter-legacy-launch.test.ts`).

**Daemon (`rust/crates/orca-daemon`)** spawns program/args verbatim and owns
the runtime half (`src/shell_ready_barrier.rs`, wired in `rpc.rs`):

- `shellArgs` present → interactive spawn; `command` is delivered via stdin
  (`command + "\n"`, terminal-host.ts semantics) through the barrier;
- `shellArgs` absent (legacy client / parity corpus) → old behavior, except
  plain sessions now default to `['-l']` (F4b);
- `shellReadySupported` → the pump scans output for the OSC 777
  `orca-shell-ready` marker (streaming scanner with partial-prefix hold,
  port of `shell-ready-marker-scanner.ts`), strips it from engine/records/
  stream, queues stdin writes while pending, and flushes through the
  post-ready gate (30ms settle after prompt bytes / 200ms wall-clock
  fallback — port of `post-ready-flush-gate.ts`), bounded by
  `shellReadyTimeoutMs` (Codex markerless: 300ms) or the 15s default;
- `createOrAttach`/`listSessions` report the live `shellState`
  (`pending`/`ready`/`timed_out`/`unsupported`);
- barrier sessions feed the engine DECODED text for their whole lifetime
  (barrier-less sessions keep the raw-bytes feed): the boundary-carrying utf8
  decoder can hold a split multibyte char as carry across the scan→post-scan
  transition, and switching the engine back to raw bytes there would hand it
  orphan continuation bytes — one corrupted glyph in reattach/checkpoint
  snapshots vs the (correct) records/stream;
- teardown `takePendingOutput` releases held partial-marker bytes as a
  post-checkpoint record (session.ts `prepareForFinalSnapshot`).

Lock order (daemon): registry → barrier → engine; the pump takes each lock
alone. Flush and timeout drain the queue under the registry lock so no
concurrent `write` can interleave with the buffered startup command.

## Windows

Untouched. The Rust daemon's transport is unix-only; Windows runs the Node
daemon, which computes its own launch args (`pty-subprocess.ts`), so
`buildPosixDaemonShellLaunch` returns null there and no `shellArgs` is sent.

## Deliberate non-ports

- **No unix shell fallback chain on spawn failure** (local provider's
  zsh→bash→sh walk): the Node daemon has no such fallback either — a bad
  `shellOverride` surfaces as a `createOrAttach` error, which is the wire
  behavior the renderer already handles. Bug-for-bug parity beats silent
  shell substitution inside the daemon.
- **`startupCommandDeliveredInShellArgs`** is Windows-only machinery; on
  POSIX the command is never argv-embedded when `shellArgs` is present.
- **fish wrappers / OSC 7 emission** stay out of scope (audit F40 tracks
  adopting `aterm-shell-integration` as the single wrapper source).

## Protocol namespace (G3)

`PROTOCOL_VERSION` is 1018 in both `types.ts` and `protocol.rs`: the fork
reserves the 1000+ namespace so its endpoints (`daemon-v1018.*`) are disjoint
from public Orca (v18) — a downgraded public build can never adopt the fork's
Rust daemon, and the fork attaches a live public Node daemon (18 is in
`PREVIOUS_DAEMON_PROTOCOL_VERSIONS`) via the legacy-adapter path instead of
impersonating it.
