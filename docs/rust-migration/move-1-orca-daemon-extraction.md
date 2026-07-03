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

1. **Spike (this move's opener).** New `orca-daemon` bin that: binds the socket,
   does the `hello` handshake at version 18, handles `createOrAttach` for **one**
   session via `orca-session`, streams `data` events, and handles
   `write`/`resize`/`kill`/`ping`. Drive it with the **existing Electron client**
   behind `ORCA_RUST_DAEMON=1` (`daemon-spawner.ts` picks the Rust bin over the
   Node entry). Success = a real shell tab runs through the Rust daemon.
2. **Full RPC surface.** Add the remaining requests (detach/reattach,
   getSnapshot, takePendingOutput, listSessions, signal, getCwd,
   getForegroundProcess, clearScrollback, health). Gate each with the **parity
   harness** (`orca-parity`): feed the same JSON request vectors to the Node and
   Rust daemons, diff the responses.
3. **Persistence + lifecycle.** Checkpoint/session-list via `orca-store`; daemon
   health socket; graceful `shutdown`; crash-restart with session restore.
4. **Cutover.** Flip the default `daemon-spawner.ts` target to the Rust bin;
   keep the Node daemon one release behind a kill-switch; delete it after a
   green release. The **autoformalization pipeline** (ts2rust two-witness) carries
   the bounded, testable request-handler logic where a hand-port is riskier.

## How this is verified (no GitHub CI — the agent-runnable gates)

- **Differential parity** (`orca-parity` + `tools/parity`): the same request
  corpus through both daemons must produce identical responses/events. This is
  the primary gate — the wire is the contract.
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

Scaffold `rust/crates/orca-daemon` with the module skeleton above and implement
**sub-step 1 (the spike)**: socket + `hello` + one `createOrAttach` session +
`data`/`write`/`resize`/`kill`/`ping`, reusing `orca-session` and
`orca-net::ndjson`. Wire `daemon-spawner.ts` to prefer it under
`ORCA_RUST_DAEMON=1`. When a real shell tab runs through it, sub-step 1 is done
and sub-step 2 (full RPC surface, parity-gated) begins.
