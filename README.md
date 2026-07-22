<h1 align="center">
  <img src="resources/build/icon.png" alt="Orca" width="64" valign="middle" /> Orca: ALab Edition
</h1>

<p align="center">
  <strong>A performance- and correctness-focused edition of Orca.</strong>
</p>

Orca: ALab Edition is an experimental downstream fork of
[Stably's Orca](https://github.com/stablyai/orca). It keeps Orca's agent workspace
and concentrates on the terminal stack, native hot paths, failure recovery, and
evidence-driven engineering.

> Upstream downloads install upstream Orca. Orca: ALab Edition is currently built
> from this repository.

For a guided tour of the product and ALab-specific engineering, see the
[feature walkthrough](FEATURE_WALKTHROUGH.md).

## Downloads

Desktop builds are on the
[Releases page](https://github.com/alabsystems/orca-alab/releases). macOS
builds are currently unsigned: right-click → **Open** on first launch.

ALab Edition versions itself independently. Each release documents the
upstream Orca version it is aligned to — the current release is aligned to
**upstream Orca v1.4.147**.

## What is Orca?

[Orca](https://github.com/stablyai/orca) is an open-source desktop workspace for
running CLI coding agents side by side. It brings isolated Git worktrees,
terminals, editing, an embedded browser, source control, diff review, SSH
workspaces, GitHub and Linear integrations, mobile monitoring, Computer Use, and
a scriptable CLI into one application.

Those product capabilities come from upstream Orca. See the
[upstream repository](https://github.com/stablyai/orca) and
[upstream documentation](https://www.onorca.dev/docs) for the complete product
guide, supported agents, screenshots, and official releases.

<p align="center">
  <img src="resources/readme-hero.jpg" alt="Orca running coding agents in parallel worktrees" width="960" />
</p>

## Why ALab Edition?

If you spend your day in terminals full of coding agents, this edition is
built for you:

- **Speed.** A Rust terminal engine with optimized CPU and GPU WebAssembly
  renderers keeps panes responsive under agent output floods, and predictive
  echo keeps typing instant even on slow or remote sessions.
- **Efficiency.** Focus-aware rendering QoS spends your machine's power on the
  pane you're looking at — background terminals batch and yield instead of
  burning CPU.
- **Stability.** Terminal sessions live in a Rust daemon with detach/reattach
  and session recovery, so agents keep running and their scrollback survives
  app restarts and crashes. Failure recovery is verified by end-to-end crash
  tests, not promised.
- **A terminal with personality.** aterm's cursor and typing effects — trail
  crossfades, fresh-ink typing, feathered ribbon ends, per-session matrix
  rain, and the nyan-rainbow cursor default.
- **Batteries included.** ALab package bundling ships compiler and solver
  support with the terminal, so agent workflows that build and verify code
  work out of the box.

The [feature walkthrough](FEATURE_WALKTHROUGH.md) shows each of these in the
product; the section below covers the engineering underneath.

## What ALab Edition improves

### Rust and aterm terminal stack

ALab Edition replaces the xterm.js rendering and headless-terminal dependencies
with a pinned [aterm](https://github.com/andrewyatesai/aterm) engine:

- Rust terminal state, parsing, search, selection, and scrollback
- optimized CPU and GPU WebAssembly renderers
- a shared render-worker architecture with explicit fallback paths
- a native Node-API terminal engine for the Electron main process
- a Rust terminal daemon with authenticated transport, detach/reattach, and
  session recovery
- Kitty keyboard handling, inline images, predictive echo, and terminal effects

The upstream aterm revision, ALab compatibility-patch digest, generated JavaScript
and type bindings, both WASM binaries, byte lengths, and SHA-256 hashes are pinned
together. WASM builds apply the compatibility patch in an isolated temporary
checkout, so the upstream submodule stays exact and clean. The build fails if any
part of that provenance drifts.

### End-to-end terminal performance

The fork treats PTY ingestion, daemon transport, backpressure, parsing, rendering,
and restoration as one pipeline. Its work includes bounded output queues,
acknowledgement-based flow control, frame coalescing, hidden-pane resource
controls, worker/GPU recovery, and reproducible latency and throughput tooling.

Performance depends on hardware and workload, so this README does not claim a
universal speedup. The benchmark harnesses and regression budgets used to evaluate
changes live in the repository.

### Evidence-driven compatibility

Terminal and migration work is checked through several independent systems:

- aterm-versus-xterm differential conformance corpora
- stateful Rust-versus-Node daemon protocol parity
- TypeScript-versus-Rust differential tests for migrated logic
- temporal models for PTY flow control, exit delivery, and keep-tail behavior
- explicit reliability, flake-history, renderer-size, and performance budgets

These prove specific contracts. They are not a claim that the entire application
has been formally verified.

### Incremental Rust migration

Selected shared logic and hot paths are moving to Rust behind parity tests,
including terminal services and portions of Git, transport, parsing, and crypto
logic. ALab Edition remains an Electron and React application; it is not a fully
native rewrite.

### Fork-safe development

The fork maintains isolated application data, daemon protocol, versioning, update
feed, and telemetry policy so development does not impersonate or overwrite an
upstream Orca installation. Development source launches use the visible identity
**Orca: ALab Edition** while retaining branch and worktree metadata internally.

The build also includes pinned generated artifacts, vendored offline Rust
dependencies, universal macOS native binaries, renderer chunk budgets, and strict
lint, typecheck, test, and packaging gates.

## Build and run from source

Prerequisites:

- Node.js 24
- pnpm 10
- rustup with stable Rust 1.96 or newer
- Xcode Command Line Tools for macOS native components

Clone with the aterm submodule and install dependencies:

```bash
git clone --depth 1 --recurse-submodules https://github.com/andrewyatesai/orca-alab.git
cd orca-alab
pnpm install --frozen-lockfile
pnpm dev
```

The first launch compiles the native terminal addon and Rust daemon. Later
launches reuse current artifacts.

To update an existing checkout:

```bash
git pull --ff-only
git submodule update --init --recursive
pnpm install --frozen-lockfile
pnpm dev
```

Maintainers can advance aterm to its latest `origin/main` revision and regenerate
the native and WASM artifacts together with:

```bash
pnpm bump:aterm
pnpm check:aterm-pin
```

The bump fails closed if a downstream compatibility patch no longer applies,
which forces that patch to be reviewed when upstream changes the same code.

Build and install the development CLI:

```bash
pnpm build:cli
orca-dev --help
orca-dev open
orca-dev status --json
```

If `~/.local/bin` is not on `PATH`, invoke `~/.local/bin/orca-dev` directly.

## Validation

Run the normal contributor gates:

```bash
pnpm lint
pnpm typecheck
pnpm test
pnpm build:rust-daemon
ORCA_LOCAL_BUILD=1 pnpm build
```

Deeper terminal and migration checks are available separately:

```bash
pnpm parity
pnpm parity:daemon
pnpm gauntlet
pnpm spec:protocols
pnpm bench:perf
pnpm bench:check
```

The full test and benchmark lanes are resource-intensive. Some platform,
hardware, and opt-in integration checks run only when their prerequisites are
available.

## Platform note

On macOS, the dedicated Computer Use helper requires user approval for
Accessibility and Screen Recording before it can control other applications or
capture their windows.

## Upstream and license

ALab Edition periodically incorporates work from
[upstream Orca](https://github.com/stablyai/orca), but it is an independent fork
and may not match the latest upstream commit at every point in time. Please report
edition-specific issues in [this repository](https://github.com/andrewyatesai/orca-alab/issues)
and upstream issues in the [Orca issue tracker](https://github.com/stablyai/orca/issues).

Orca: ALab Edition is distributed under the [Apache License 2.0](LICENSE),
Copyright 2026 Andrew Yates (see [NOTICE](NOTICE)). Portions derived from
upstream Orca remain Copyright (c) 2026 Lovecast Inc. under the MIT License;
the upstream notice is preserved in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
