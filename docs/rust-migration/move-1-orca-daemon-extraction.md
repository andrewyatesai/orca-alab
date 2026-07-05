# Move 1 — Extract `orca-daemon` as a Pure Rust Binary

> Status: **executing**. This is the first move of
> [`native-orca-three-move-path.md`](./native-orca-three-move-path.md). It is
> grounded in the real daemon protocol (`src/main/daemon/`) and the Rust crates
> that already exist (`rust/crates/*`), not in the aspirational sketch.

## The thesis, concretely

Today the daemon is a **Node child process** (`src/main/daemon/`, spawned by
`daemon-spawner.ts`) that owns every PTY and streams it to the Electron main
process over a Unix-domain socket. The main process reaches the terminal engine
(aterm) through **napi** on the hot path.

Inverted, the daemon is a **Rust process** that uses `aterm-core` *as a crate*
via `orca-session` — **the napi boundary disappears from the PTY→engine hot
path entirely.** Electron keeps talking to it over the **same socket protocol**,
so nothing user-visible changes. This is the lowest-risk, highest-leverage first
move.

## What already exists (the migration is 70% scaffolded)

| Daemon concern | TS today | Rust crate that replaces it | State |
| --- | --- | --- | --- |
| NDJSON wire framing | `daemon/ndjson.ts` | **`orca-net::ndjson`** (`NdjsonSplitter`, `encode_ndjson_line`) | ✅ ported + parity-tested |
| PTY spawn / IO | `node-pty` via `daemon-pty-adapter.ts` | **`orca-pty`** (`PtyCommand`, vendored `portable-pty`) | ✅ exists |
| Session = PTY + headless VT | `session/` + `@xterm/headless` | **`orca-session::TerminalSession`** (PTY → `aterm-core`) | ✅ exists (`spawn/size/cursor/row_text/cell/wait`) |
| Terminal-stream framing (remote/mobile) | `ws` relay | **`orca-relay`** | ✅ parity-proven |
| Session/checkpoint persistence | `node:sqlite` + `daemon-checkpoint-file.ts` | **`orca-store`** (bundled SQLite) | ✅ exists |
| SSH remote runtime | `src/main/ssh` | **`orca-ssh`** | ⏳ config-parse cut; transport pending |
| Proof the stack runs | — | **`orca-aterm-demo`** (127 LOC: live PTY → aterm → grid via `orca-session`) | ✅ **this is the proto-spike** |

`orca-aterm-demo/src/main.rs` already does the hard half — spawn a real PTY
through `orca-session`, stream the child's bytes through aterm's VT parser, read
back the grid. Move 1 is **that, wrapped in the daemon's socket protocol.**

## The contract Move 1 must preserve (byte-exact)

From `src/main/daemon/types.ts` — the wire the Electron client
(`daemon-pty-provider.ts` → `daemon-client`) already speaks. **The Rust daemon
must be indistinguishable from the Node one at this boundary.**

- **Transport**: Unix-domain socket, `chmod 0o600`. **NDJSON** framing
  (newline-delimited JSON; resync-on-newline — already in `orca-net::ndjson`).
- **`PROTOCOL_VERSION = 18`** (+ `PREVIOUS_DAEMON_PROTOCOL_VERSIONS` accepted for
  rolling upgrades).
- **Handshake**: first line on each socket is a `hello`
  `{ version, token, clientId, role: 'control' | 'stream' }`; daemon replies
  `{ type: 'hello', ok, error? }`. Each client opens **two** sockets — a
  **control** socket (RPC) and a **stream** socket (output), correlated by
  `clientId`.
- **RPC requests** (control socket, id-correlated `RpcResponse` ok/error):
  `createOrAttach`, `cancelCreateOrAttach`, `write`, `resize`, `kill`, `signal`,
  `listSessions`, `detach`, `getCwd`, `getForegroundProcess`, `clearScrollback`,
  `shutdown`, `ping`, `systemResolverHealth`, `ptySpawnHealth`, `getSnapshot`,
  `getSize`, `takePendingOutput`.
- **Events** (daemon → client, stream socket): `data` (session bytes), `exit`
  (code), `terminalError`.
- **Session model**: `SessionState = created | spawning | running | exiting |
  exited`; `SessionInfo` / `DaemonSessionInfo`; sessions **survive client
  disconnect** (detach/reattach — this is why the daemon is a separate process at
  all). `getSnapshot` replays scrollback on reattach.

The subtle, must-not-regress behaviors: **createOrAttach idempotency** (attach to
a live session vs create), **detach keeps the PTY alive**, `takePendingOutput`
drains buffered bytes for a reattaching client, `shellReady` timing, and the
Windows shell-override resolution (`shellOverride`,
`terminalWindowsPowerShellImplementation`, WSL distro) — all encoded in
`CreateOrAttachRequest.payload`.

## Target architecture — `rust/crates/orca-daemon`

A new binary crate composing what exists:

```
orca-daemon (bin)
├── transport/          UnixListener + per-socket NdjsonSplitter (orca-net)
│   ├── hello handshake (version/token/clientId/role)
│   └── control ⇄ stream socket pairing by clientId
├── rpc/                DaemonRequest → handler; id-correlated RpcResponse
├── session_registry/   sessionId → orca_session::TerminalSession (survives detach)
│   └── output pump: PTY bytes → `data` events on the stream socket
├── persistence/        checkpoint + session list via orca-store (SQLite)
└── health/             ping / ptySpawnHealth / systemResolverHealth
```

- **Runtime**: single-threaded `tokio` (or a hand-rolled `mio` loop to stay
  vendored-offline — decide in the spike; `orca-pty`'s read side is blocking
  today, so a reader thread per session + an mpsc into the socket writer is the
  low-dependency baseline).
- **One `TerminalSession` per session id**, held in the registry across client
  disconnects. The output pump is the same drain `orca-aterm-demo` does, but
  streamed continuously as `data` events instead of read once at exit.
- **`orca-store`** holds the session list + checkpoint so a daemon restart (or
  `getSnapshot` reattach) restores scrollback — parity with
  `daemon-checkpoint-file.ts`.

## Phased sub-steps (each independently shippable behind a flag)

1. ✅ **Spike (this move's opener) — DONE.** `orca-daemon` bin binds the socket,
   does the `hello` handshake at version 18, handles `createOrAttach`, streams
   `data`/`exit` events, and handles `write`/`resize`/`kill`/`ping`. Builds offline
   (rustc 1.96, vendored) and passes an end-to-end smoke test.
2. 🟢 **Full RPC surface + session lifecycle + engine queries — DONE.** Reattach
   (`createOrAttach` on a live id → `isNew:false`), **pending-output buffering**
   (output produced while detached is buffered per session and replayed on stream
   reconnect), `takePendingOutput`, real `listSessions`, `getSize`, `detach`,
   `shutdown`, **real `signal`**, and safe stubs for `cancelCreateOrAttach`/health. **Plus the
   showcase**: each session tees its raw PTY output into an `orca-terminal`/aterm
   `HeadlessTerminal`, so `getSnapshot` (real `snapshotAnsi`/`scrollbackAnsi` +
   `modes` + `cwd` + dims), `getCwd` (OSC-7), and `clearScrollback` are answered
   from **actual aterm engine state — no napi hop**. Two smoke tests pass: an
   8-check reattach/buffering suite and an engine suite (OSC-7 cwd round-trip +
   rendered-grid snapshot). **The differential parity gate is now live** — see
   the next section. **`getForegroundProcess` is real too** — it resolves the PTY's
   foreground process group (`tcgetpgrp`) to the child's command name, mirroring
   node-pty's `.process` (null only when the pgid is gone).
3. 🟡 **Persistence + lifecycle.** The exited-session **reaper is done**
   (`reap_and_mark_exited` — a child exit removes the entry, so `listSessions`
   never shows zombies and a reattach to an exited id spawns fresh). Still pending:
   checkpoint/session-list persisted **in the daemon** via `orca-store` (records are
   drained to the client today, not stored daemon-side), a daemon health socket, and
   crash-restart with session restore.
4. 🟢 **Cutover — DONE on macOS/Linux (flagless).** The Rust bin is now THE daemon
   on Unix with **no env flag and no Node fallback** (`daemon-init.ts`
   `rustDaemonEnabled()` = `platform !== 'win32'`; only `ORCA_RUST_DAEMON_BIN`
   overrides the binary path). There is **no Unix kill-switch** — a missing binary or
   startup timeout throws and degrades the app to the in-process, non-persistent
   `LocalPtyProvider`. Windows keeps the Node named-pipe daemon until the Unix-socket
   transport gets a Windows twin (Rust `serve` is a `not(unix)` `Unsupported` stub).
   The **autoformalization pipeline** (ts2rust two-witness) carries the bounded,
   testable request-handler logic where a hand-port is riskier.

## The differential parity gate — LIVE (`tools/daemon-parity`, `pnpm parity:daemon`)

The primary safety gate now exists and is **two-legged and live**. It drives one
stateful RPC corpus over the **real Unix socket** (hello handshake, control +
stream pairing by `clientId`, NDJSON framing, event delivery) against **both**
daemons and diffs their structural fingerprints:

- **Leg A (hard gate)** — the Rust `orca-daemon` must satisfy 15 behavioral
  invariants (create/isNew, OSC-7 `getCwd`, snapshot dims+cwd+marker, `getSize`,
  `listSessions` alive, live stream carries the marker, reattach `isNew:false`,
  resize, unknown-session errors, kill → not-alive). This is also the first
  coverage of the socket transport itself — `tests/rpc_lifecycle.rs` calls
  `dispatch_request` directly and bypasses hello/pairing/events.
- **Leg B (differential)** — the Node daemon (`out/main/daemon-entry.js` spawned
  headless via `ELECTRON_RUN_AS_NODE=1`) runs the **same** corpus; any structural
  divergence fails the gate. If it can't be spawned, the leg is loudly skipped.

What is compared is **wire structure + behavior**, not engine-render bytes (the
two daemons legitimately render through different VT engines). Its first run
caught four real Rust wire drifts (`getCwd` bare-string, `getSize` top-level
dims, `getForegroundProcess`/`ptySpawnHealth` envelopes, missing `sgrMouse*`
modes) — all fixed; the gate is now green (15/15 invariants, Node == Rust across
all 15 steps). See `tools/daemon-parity/README.md`.

## How this is verified (no GitHub CI — the agent-runnable gates)

- **Differential parity** (`tools/daemon-parity`, above): the same request corpus
  through both daemons must produce identical structural responses/events. This
  is the primary gate — the wire is the contract.
- **`orca-aterm-demo` stays green**: the session-through-aterm proof.
- **Conformance/perf/safety gauntlet**: the Rust daemon must clear the existing
  gates (it removes a process boundary + the napi hop, so perf should *improve*).
- **The existing daemon test suite** (`daemon-server.test.ts`,
  `daemon-pty-provider.test.ts`, `daemon-pty-adapter.test.ts`, …) becomes the
  behavioral oracle for the parity vectors.

## Risks & the honest hard parts

- **PTY portability**: `portable-pty` (via `orca-pty`) must match node-pty on
  Windows ConPTY + the shell-override matrix. Windows is the highest-risk surface;
  land macOS/Linux first, keep the Node daemon for Windows until parity proven.
- **Blocking PTY reads vs the socket writer**: needs a clean reader-thread ⇄
  writer seam; get it right in the spike so the whole crate inherits it.
- **`shellReady` / startup-command delivery** timing is subtle and user-visible
  (prompt races) — port it against the daemon tests, not from memory.
- **Offline vendoring**: the daemon crate must build from `rust/vendor`; a tokio
  pull-in would break the offline build. Prefer std + `mio` (already a transitive
  dep) or justify the vendored async runtime in the spike.

## Immediate next action

Sub-steps 1, 2, and the **Unix cutover** (sub-step 4) are done: the crate builds
offline, embeds the real aterm engine per session, passes the in-process lifecycle
tests, **clears the live two-legged differential parity gate** (`pnpm parity:daemon`,
15/15, Node == Rust), and now **ships as THE macOS/Linux daemon with no flag** (the
spawner launches it unconditionally; `getForegroundProcess`/`signal`/the
exited-session reaper are all real). Next, in order:

1. **Grow the parity corpus.** Add vectors for `takePendingOutput` while detached
   (buffer-then-replay), multi-client `clientId` isolation, `detach` keeping the
   PTY alive, `shellReady`/startup-command timing, and the Windows shell-override
   matrix. Each new vector that stays green is another degree of cutover safety.
2. **Windows transport.** Give the `not(unix)` `serve` a real listener (named pipe
   or AF_UNIX) so Windows can drop the Node daemon too.
3. **In-daemon persistence + crash-restart** (sub-step 3): persist checkpoint/
   session-list daemon-side via `orca-store`; restore sessions after a daemon crash.
4. **OSC-133 shell-readiness** — detect shell-ready in the engine instead of
   reporting the honest `unsupported` `ShellReadyState`.
