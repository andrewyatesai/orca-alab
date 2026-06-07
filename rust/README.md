# Orca native Rust workspace

The cross-platform Rust core of the native Orca rewrite. See
[`docs/rust-migration/`](../docs/rust-migration/) for the architecture,
functional map, dependency map, migration plan, and the ported-modules ledger.

## Layout

```
rust/
├── Cargo.toml            # workspace (release profile: opt-level=z, LTO, strip)
└── crates/
    └── orca-core/        # pure cross-cutting logic ported from src/shared (no IO)
```

Higher tiers (`orca-git`, `orca-pty`, `orca-ssh`, `orca-runtime`, `orca-terminal`,
`orca-ffi`, …) are added per the migration plan. The terminal engine lives in
the separate `aterm` repo and is consumed by `orca-terminal`.

## Build & test

```sh
cargo test  --manifest-path crates/orca-core/Cargo.toml   # behavioural parity vs the TS tests
cargo clippy --manifest-path crates/orca-core/Cargo.toml --all-targets
cargo build --release                                     # stripped, LTO'd
```

`orca-core` is zero-dependency, `#![forbid(unsafe_code)]`, and written
panic-free so it can be verified with **Trust** ("trusted Rust") once a stage2
sysroot is built:

```sh
# from a Trust stage2 sysroot (see ~/trust):
tcargo trust check --format json --manifest-path crates/orca-core/Cargo.toml
```

## Porting invariant

Every module is a faithful port of its `src/shared/*` source **with the original
test cases translated verbatim**, so `cargo test` is the parity gate. See the
ledger for what's done and what's next.
