<h1 align="center">
  <img src="resources/build/icon.png" alt="Orca" width="64" valign="middle" /> Orca: ALab Edition
</h1>

<p align="center">
  <strong>An IDE for orchestrating fleets of AI coding agents — built on a Rust terminal engine.</strong>
</p>

Orca: ALab Edition is a desktop workspace for running CLI coding agents side
by side: isolated Git worktrees, terminals, editing, an embedded browser,
source control, diff review, SSH workspaces, GitHub and Linear integrations,
mobile monitoring, Computer Use, and a scriptable CLI — with the terminal stack,
native hot paths, and failure recovery rebuilt around the
[aterm](https://github.com/alabsystems/aterm) Rust terminal engine.

Guided tour: the [feature walkthrough](FEATURE_WALKTHROUGH.md).

## Downloads

Desktop builds are on the
[Releases page](https://github.com/alabsystems/orca-alab/releases); cuts ship
macOS assets by default, and a multi-platform cut is an explicit opt-in. macOS
builds are ad-hoc signed for launch, not Developer ID-signed or notarized:
right-click → **Open** on first launch. Squirrel.Mac cannot update an ad-hoc-signed
app, so macOS updates in place by swapping the `.app` bundle itself, verified by
SHA-512 over the artifact plus an Ed25519 feed signature when a public key was
pinned at build time. Windows and Linux update by manual reinstall from that page.

ALab Edition versions itself independently on a `MAJOR.MINOR.0` line, currently
**0.3.0**; the older `<upstream>-fork.N` tags (last `v1.4.147-fork.1`) are
retired, because that number was upstream's and SemVer read the suffix as a
prerelease. The tree is aligned to upstream Orca **v1.4.165-rc.0**.

<p align="center">
  <img src="resources/readme-hero.jpg" alt="Orca running coding agents in parallel worktrees" width="960" />
</p>

## Why ALab Edition?

- **Speed.** Optimized CPU and GPU WebAssembly renderers keep panes responsive
  under agent output floods, and predictive echo keeps typing instant on slow or
  remote sessions.
- **Efficiency.** Focus-aware render QoS spends your machine's power on the pane
  you're looking at: the shared render worker gives it an 8:1 share of the drain
  while still guaranteeing every background pane a turn.
- **Stability.** Sessions live in a Rust daemon with detach/reattach and session
  recovery, so agents keep running and their scrollback survives app restarts and
  crashes — verified by end-to-end crash tests, not promised.
- **A terminal with personality.** Six cursor-trail styles (the theme-derived
  `water` trail is the default), sparkle-word decorations, and opt-in per-session
  phosphor rain, all respecting the OS reduce-motion preference.
- **Modes.** Settings pick one of three shells over the same engine: Orca Classic
  (the default), ALab (a supervisory console over a fleet of agent terminals), and
  Story World. A mode gates, places, and rewords existing surfaces — it never owns
  or mutates engine state, and a test holds Classic to "today's product, unchanged."

## What ALab Edition improves

### Rust and aterm terminal stack

The xterm.js rendering and headless-terminal dependencies are gone, replaced by a
pinned aterm engine:

- Rust terminal state, parsing, search, selection, and scrollback
- optimized CPU and GPU WebAssembly renderers
- a shared render-worker architecture with explicit fallback paths
- a native Node-API terminal engine for the Electron main process
- a Rust terminal daemon with authenticated transport, detach/reattach, and
  session recovery
- Kitty keyboard handling, inline images, predictive echo, and terminal effects

The upstream revision, ALab compatibility-patch digest, generated JavaScript and
type bindings, both WASM binaries, byte lengths, and SHA-256 hashes are pinned
together, and the build fails if any part of that provenance drifts. WASM builds
apply the patch in a detached throwaway worktree, so the submodule stays clean.

PTY ingestion, daemon transport, backpressure, parsing, rendering, and restoration
are treated as one pipeline: bounded output queues, acknowledgement-based flow
control, frame coalescing, hidden-pane resource controls, and worker/GPU recovery.
Performance depends on hardware and workload, so this README claims no universal
speedup; the benchmark harnesses and regression budgets used to evaluate changes
live in the repository.

### Evidence-driven verification

Terminal and migration work is checked through several independent systems:

- aterm-versus-xterm differential conformance corpora
- stateful Rust-versus-Node daemon protocol parity
- TypeScript-versus-Rust differential tests for migrated logic
- temporal models for PTY flow control, exit delivery, keep-tail behavior, and
  echo liveness
- machine-checkable `ay` proof certificates, shipped today by nine crates and
  discharged by the `certificates` gauntlet axis
- explicit reliability, flake-history, renderer-size, and performance budgets

The Rust workspace builds with **Trust**, a verifying Rust toolchain.
`rust-toolchain.toml` pins the `trust` channel, and every build and test script
runs `targo`, Trust's cargo. Routine builds (Rust daemon, native addon, parity,
`pnpm test:rust`) use its unverified lane, `targo --unverified`, which makes no
proof claim — so the packaged binaries are not verified builds.
`pnpm verify:rust` is the verified lane (`targo trust check`); it reports per
crate rather than gating, and its advisory verdicts are **verified-and-reported,
not verified-clean**. The WebAssembly blobs, the x86_64 slice of a universal
macOS build, and Linux/Windows builds still use stock Rust, because the Trust
toolchain ships a standard library only for Apple-silicon macOS today. All of
this proves specific contracts, not that the whole application is formally
verified.

### Incremental Rust migration

Shared logic and hot paths are moving to Rust behind parity tests, across 27
first-party crates covering terminal services and portions of Git, transport,
parsing, policy, and crypto. ALab Edition remains an Electron and React
application; it is not a fully native rewrite.

### Fork-safe development

Application data, daemon protocol, versioning, update feed, and telemetry policy
are isolated, so development never overwrites an upstream Orca installation, and
source launches carry the visible identity **Orca: ALab Edition**. The build also
carries pinned generated artifacts, vendored offline Rust dependencies, universal
macOS native binaries, renderer chunk budgets, and strict lint, typecheck, test,
and packaging gates.

## Build and run from source

Prerequisites:

- Node.js 24
- pnpm 10
- the Trust Rust toolchain (`targo`/`trustc`, installed by atpkg) on Apple-silicon
  macOS; on other hosts, rustup with an installed stable Rust 1.96 or newer
- Xcode Command Line Tools for macOS native components

`rust-toolchain.toml` pins the `trust` channel, and the build scripts run
`targo --unverified`. The scripts never install a Rust toolchain: where stock Rust
is still required (WebAssembly, the x86_64 macOS slice, Linux/Windows hosts) they
use an installed rustup toolchain (`ORCA_STOCK_RUST_TOOLCHAIN`, default `stable`)
and fail with the install command if it is missing.

Clone with the aterm submodule and install dependencies:

```bash
git clone --depth 1 --recurse-submodules https://github.com/alabsystems/orca-alab.git
cd orca-alab
pnpm install --frozen-lockfile
pnpm dev
```

The first launch compiles the native terminal addon and Rust daemon; later
launches reuse current artifacts. Update an existing checkout with `git pull
--ff-only && git submodule update --init --recursive`, then re-run the last two
commands above.

Maintainers can advance aterm to its latest `origin/main` revision and regenerate
the native and WASM artifacts together with:

```bash
pnpm bump:aterm
pnpm check:aterm-pin
```

That path also needs an installed stock stable toolchain with the
`wasm32-unknown-unknown` target (Trust ships no wasm32 standard library) and Binaryen's
`wasm-opt` on `PATH` (`brew install binaryen` on macOS). The bump refreshes both
Cargo lockfiles, the native addon, the Rust daemon, and the committed WASM
artifacts, and fails closed if a downstream compatibility patch no longer applies
— which forces that patch to be reviewed when upstream touches the same code.

Build and install the development CLI:

```bash
pnpm build:cli
orca-dev --help
orca-dev status --json
```

If `~/.local/bin` is not on `PATH`, invoke `~/.local/bin/orca-dev` directly.

## Validation

```bash
# contributor gates
pnpm lint && pnpm typecheck && pnpm test && pnpm test:rust
pnpm build:rust-daemon && ORCA_LOCAL_BUILD=1 pnpm build

# deeper terminal, migration, and verification lanes
pnpm parity && pnpm parity:daemon && pnpm gauntlet && pnpm spec:protocols
pnpm bench:perf && pnpm bench:check
pnpm verify:rust
```

The deeper lanes are resource-intensive, and platform, hardware, and opt-in checks
run only when their prerequisites are available.

## Platform note

On macOS, the dedicated Computer Use helper requires user approval for
Accessibility and Screen Recording before it can control other applications or
capture their windows; `orca computer permissions` reports and opens that setup.
Because ALab builds use ad-hoc signatures, macOS may ask you to approve privacy
permissions again after installing a rebuilt or newer ALab build.

## License and attribution

Orca: ALab Edition is distributed under the [Apache License 2.0](LICENSE),
Copyright 2026 Andrew Yates (see [NOTICE](NOTICE)).

It incorporates code from the [Orca project](https://github.com/stablyai/orca),
Copyright (c) 2026 Lovecast Inc., under the MIT License; that notice and all
third-party notices are preserved in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md). Report issues in
[this repository](https://github.com/alabsystems/orca-alab/issues).
