# Orca native Rust workspace

The cross-platform Rust core of the native Orca rewrite. The layout, build/test
commands, and the porting invariant are below; the architecture, functional and
dependency maps, migration plan, and ported-modules ledger live in the internal
`docs/rust-migration/` notes, which are not part of the public source snapshot.

## Layout

```
rust/
├── Cargo.toml            # workspace (release profile: opt-level=z, LTO, strip)
├── aterm/                # terminal engine submodule, consumed by orca-terminal
└── crates/               # 27 crates, all building today
    └── orca-core/        # pure cross-cutting logic ported from src/shared (no IO)
```

All 27 crates now exist and build: the pure/config tiers (`orca-core`, `orca-text`,
`orca-config`, `orca-agents`, `orca-policy`), the IO tiers (`orca-git`, `orca-pty`,
`orca-ssh`, `orca-net`, `orca-store`, `orca-crypto`, `orca-relay`, `orca-runtime`,
`orca-winpipe`), the terminal stack (`orca-terminal`, `orca-session`, `orca-session-gc`,
`orca-ffi`, `orca-aterm-demo`, `orca-flow-control`, `orca-stream-split`,
`orca-renderer-heap`), the resilience tier (`orca-crash-recovery`, `orca-dispatch`,
`orca-provider-backoff`), and the native `orca-daemon` binary with its `orca-parity`
harness. `targo --unverified test --workspace` passes for the buildable set
(`pnpm test:rust` from the repo root). The terminal engine lives in the `aterm` submodule (`rust/aterm`) and is
consumed by `orca-terminal`.

**The engine pin is a commit, not a version string.** aterm's version line was
restarted, and the version values reachable from this tree disagree with one another,
so no number here would be trustworthy. The authoritative pin is `sourceCommit` in
`src/renderer/src/lib/pane-manager/aterm/aterm_wasm_artifact_pin.json`, enforced
offline by `pnpm check:aterm-pin`, which requires `[workspace.package] version` in
`rust/aterm/Cargo.toml` to match the `aterm(x.y.z)` marker embedded in each committed
WASM blob.

## Build & test

The workspace builds with the Trust toolchain (`rust-toolchain.toml` pins
`trust`); `targo` is its cargo, and every command names its lane:

```sh
targo --unverified test -p orca-core               # behavioural parity vs the TS tests
targo tippy -p orca-core --all-targets             # lint
targo --unverified build --release                 # stripped, LTO'd (no proof claim)
targo trust check -p orca-core --format json       # verified lane (fail-closed)
```

`rust/.cargo/config.toml` holds only the offline vendoring; verification policy
belongs to the lane, not to a config table. `targo trust` refuses a
world-writable `CARGO_TARGET_DIR` (for example under `/tmp`).

`orca-core` is zero-dependency, `#![forbid(unsafe_code)]`, and written
panic-free so it can be verified with **Trust** ("trusted Rust").

## Porting invariant

Every module is a faithful port of its `src/shared/*` source **with the original
test cases translated verbatim**, so `targo --unverified test` is the parity gate. See the
ledger for what's done and what's next.
