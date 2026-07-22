# Repo-specific transforms applied to the export tree (sourced by publish.sh
# with $EXPORT and $OUT set, and fail/note available). Each transform must
# verify it applied and fail loudly when stale.

# T1: replace the dev README with the public landing README. The dev README
# documents building from a full source checkout; this snapshot ships only the
# landing files, so those instructions would be non-functional here (2026-07-22
# release audit). The public page must also never reference the private dev
# org (the central rewrite + guard forbid it), so downloads point at THIS
# repo's Releases, which mirror the binary release.
python3 - "$EXPORT" <<'PY'
import sys
export = sys.argv[1]
path = f"{export}/README.md"
with open(path) as f:
    dev = f.read()
assert "Build and run from source" in dev, "transform T1 stale: dev README shape changed"

public = '''<h1 align="center">
  <img src="resources/build/icon.png" alt="Orca" width="64" valign="middle" /> Orca: ALab Edition
</h1>

<p align="center">
  <strong>A performance- and correctness-focused edition of Orca.</strong>
</p>

Orca: ALab Edition is an experimental downstream fork of
[Stably's Orca](https://github.com/stablyai/orca). It keeps Orca's agent
workspace and concentrates on the terminal stack, native hot paths, failure
recovery, and evidence-driven engineering.

This repository carries the fork's **reviewed public snapshots and releases**.
Development happens in the fork's development repository; each snapshot here
is exported, scanned, and verified by the fork's publication pipeline before
it lands.

<p align="center">
  <img src="resources/readme-hero.jpg" alt="Orca running coding agents in parallel worktrees" width="960" />
</p>

## Downloads

Desktop builds live on this repository's
[Releases page](https://github.com/alabsystems/orca-alab/releases).

> macOS builds are currently **unsigned**: Gatekeeper requires right-click →
> Open on first launch. Upstream's own downloads at
> [onorca.dev](https://www.onorca.dev) install upstream Orca, not this
> edition.

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

The terminal engine is **aterm**, a Rust terminal built for AI workloads; its
public source publication is being prepared separately.

## What is Orca?

[Orca](https://github.com/stablyai/orca) is an open-source desktop workspace
for running CLI coding agents side by side: isolated Git worktrees, terminals,
editing, an embedded browser, source control, diff review, SSH workspaces,
GitHub and Linear integrations, mobile monitoring, Computer Use, and a
scriptable CLI. Those product capabilities come from upstream Orca — see the
[upstream repository](https://github.com/stablyai/orca) and
[upstream documentation](https://www.onorca.dev/docs) for the complete product
guide.

## Versions

This repository uses two version lines, on purpose:

- **Snapshot tags** (`v0.x.0`) version the reviewed public snapshots of this
  repository.
- **App versions** (`1.4.x-fork.N`) version the desktop builds themselves,
  tracking the upstream Orca version they incorporate. A release here names
  both.

## License

Orca: ALab Edition is distributed under the
[Apache License 2.0](LICENSE), Copyright 2026 Andrew Yates (see
[NOTICE](NOTICE)). Portions derived from upstream Orca remain Copyright (c)
2026 Lovecast Inc. under the MIT License; the upstream notice and all
third-party notices are preserved in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
'''

with open(path, "w") as f:
    f.write(public)
PY
note "transform T1 applied: public landing README swapped in"
