# Daemon parity gate — Rust `orca-daemon` vs. the Node daemon, over the real wire

Move 1 of the native migration ([`docs/rust-migration/move-1-orca-daemon-extraction.md`](../../docs/rust-migration/move-1-orca-daemon-extraction.md))
replaces the Node terminal daemon (`src/main/daemon/`) with a pure-Rust one
(`rust/crates/orca-daemon`). The contract is the **Unix-socket NDJSON protocol**
(`src/main/daemon/types.ts`, `PROTOCOL_VERSION = 18`) — the Electron client must
not be able to tell which daemon it is talking to. This harness is the gate that
proves that, by driving **one stateful RPC corpus over the real socket** against
**both** daemons and diffing their responses.

Unlike `tools/parity` (pure-function ports of `src/shared`), the daemon is
stateful and lives in `src/main`, so it needs its own harness shape.

## Run it

```bash
pnpm parity:daemon        # == node tools/daemon-parity/run.mjs
```

Prereqs:

- **Rust leg:** the `orca-daemon` binary built at `rust/target/{debug,release}/orca-daemon`:
  ```bash
  PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" \
    cargo build -p orca-daemon --manifest-path rust/Cargo.toml --offline
  ```
- **Node leg (optional):** `out/main/daemon-entry.js` (`pnpm build:electron-vite`)
  and the Electron binary. Spawned headless via `ELECTRON_RUN_AS_NODE=1` so
  node-pty's native ABI matches.

## Two legs

- **Leg A — Rust `orca-daemon` (hard gate).** Spawns the binary, opens a real
  control + stream socket pair (`hello` handshake, shared `clientId`, NDJSON
  framing), drives the corpus, and asserts a set of **behavioral invariants**.
  This is also the first coverage of the socket transport itself — the in-process
  `rust/crates/orca-daemon/tests/rpc_lifecycle.rs` tests call `dispatch_request`
  directly and bypass hello / socket pairing / event delivery.
- **Leg B — Node daemon (differential).** If it can be spawned in this
  environment, the **same** corpus runs against it and its structural
  fingerprint is diffed against Rust's; any divergence **fails** the gate. If it
  cannot be spawned, the leg is **loudly skipped** (never silently passed) and
  the Rust invariants remain the gate.

## What is compared — and what is deliberately not

The two daemons render through **different VT engines** (Rust aterm vs Node
`@xterm/headless`), so byte-exact `snapshotAnsi` is *not* a parity goal — that is
the aterm conformance gauntlet's job. This gate compares **wire-protocol
structure + behavior**: response shapes (`{cwd}`, `{size:{cols,rows}}`,
`{snapshot}`, …), `ok`/`error`, field presence and types, `isNew` idempotency,
event framing, and semantic facts (marker present in the rendered snapshot, cwd
parsed from OSC-7, dims honored). Volatile/engine values (pid, `createdAt`,
rendered bytes) are reduced to type-tags / booleans before diffing
(`request-vectors.mjs`).

## Layout

```
tools/daemon-parity/
  daemon-socket-client.mjs  NDJSON control+stream client (hello, id-correlated RPC, events)
  request-vectors.mjs       the stateful corpus + per-step structural projection
  run.mjs                   spawns each daemon, drives the corpus, checks invariants + diffs
```

## What it has already caught

The first live run surfaced four real wire drifts in the Rust daemon, since
fixed: `getCwd` returned a bare string instead of `{ cwd }`; `getSize` returned
the dims at the payload top level instead of `{ size: { cols, rows } }`;
`getForegroundProcess` / `ptySpawnHealth` used the wrong envelope; and the
snapshot `modes` omitted `sgrMouseMode` / `sgrMousePixelsMode`. That is the point
of the gate: catch wire divergence before the cutover, when it is invisible.
