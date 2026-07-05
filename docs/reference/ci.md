# Fork CI

CI for this fork was stood up from zero (staging-launch audit **F15**/**F16**,
`docs/reference/staging-launch-audit-2026-07-04.md`). Upstream's 11 workflows
were deliberately deleted with the fork; these are lean fork gates only — no
upstream release trains, notarization, or mobile automation.

Workflows live in `.github/workflows/`:

| File              | Trigger                          | Purpose                                  |
| ----------------- | -------------------------------- | ---------------------------------------- |
| `pr.yml`          | `pull_request`, `push` to `main` | Merge gates (typecheck → e2e)            |
| `release-mac.yml` | `workflow_dispatch` only         | Manual mac packaging + arch assertion    |

## One-time repository setup

- **`ATERM_SUBMODULE_TOKEN` secret (required).** `rust/aterm` is a git
  submodule pointing at a separate private repo
  (`andrewyatesai/aterm`). The job-scoped `GITHUB_TOKEN` can only read the
  current repo, so every checkout uses
  `${{ secrets.ATERM_SUBMODULE_TOKEN || github.token }}`. Create a fine-grained
  PAT with read-only **Contents** access to the aterm repo and add it as an
  Actions secret named `ATERM_SUBMODULE_TOKEN`. Without it, checkout fails at
  the submodule fetch with a clear error (nothing is silently skipped).
- No other secrets are needed. Release builds are intentionally unsigned
  (`CSC_IDENTITY_AUTO_DISCOVERY=false` → ad-hoc signatures).

## `pr.yml` jobs

Durations are estimates until real runs establish baselines: **cold** = empty
caches, **warm** = pnpm store + `Swatinem/rust-cache` hits.

### `core` — ubuntu-latest (~30 min cold / ~15 min warm)

| Step                      | Local equivalent                |
| ------------------------- | ------------------------------- |
| Build aterm napi addon    | `pnpm run build:terminal-addon` |
| Typecheck (node+cli+web)  | `pnpm tc`                       |
| Lint (oxlint + gates)     | `pnpm lint`                     |
| Full vitest suite         | `pnpm test`                     |

Notes:

- `pnpm lint` includes `check:aterm-pin`, which reads the submodule's
  `Cargo.toml` — hence `submodules: true` on checkout.
- The addon build needs cargo (rustup stable ≥ 1.96): the addon's crates.io
  deps are lockfile-pinned and fetched from the network; the `rust/` workspace
  crates it path-deps resolve offline from `rust/vendor`.
- `pnpm test` re-runs the addon build, but the freshness probe makes it a
  no-op after the explicit build step.

### `rust` — ubuntu-latest (~25 min cold / ~8 min warm)

| Step                              | Local equivalent                                 |
| --------------------------------- | ------------------------------------------------ |
| `cargo check --workspace`         | `(cd rust && cargo check --workspace)`           |
| `cargo test -p orca-daemon -p orca-pty` | `(cd rust && cargo test -p orca-daemon -p orca-pty)` |

The workspace is offline-by-construction (`rust/.cargo/config.toml` resolves
everything from `rust/vendor`), so this job never touches crates.io.

### `daemon-parity` — macos-15 (~30 min cold / ~15 min warm)

The "client cannot tell which daemon" launch guarantee. Runs
`node tools/daemon-parity/run.mjs`:

- **Leg A (hard gate):** release `rust/target/release/orca-daemon` built via
  `pnpm run build:rust-daemon`, driven over the real Unix socket against the
  behavioral invariant checklist.
- **Leg B (differential):** the Node daemon (`out/main/daemon-entry.js` from
  `node config/scripts/run-electron-vite-build.mjs`, spawned via
  electron-as-node). The job builds `orca_node.node` first so this leg
  actually runs instead of loudly skipping; any Rust↔Node divergence fails.

macOS runner because the parity harness resolves the electron binary at the
`Electron.app` path and both daemons speak Unix sockets. Local equivalent:
`pnpm run build:rust-daemon && pnpm run build:terminal-addon && pnpm run build:electron-vite && pnpm run parity:daemon`.

### `e2e` (shards 1–4) — macos-15 (~45–70 min per shard)

`pnpm run test:e2e --shard=N/4` (no `--` separator — pnpm 10 appends trailing
flags verbatim, and a literal `--` would demote `--shard` to a playwright
positional filter) — the full default Playwright suite
(132 spec files, `electron-headless` project). Prerequisites built first:

- `pnpm run build:rust-daemon` — on macOS the Rust daemon is THE terminal
  daemon with no Node fallback; without the dev binary
  (`rust/target/release/orca-daemon`) the app silently degrades to the
  in-process provider and daemon/terminal specs test the wrong thing.
- `pnpm run build:terminal-addon` — `orca_node.node` (main-process git
  parsers + headless emulator).
- The Electron app itself is built by Playwright's `globalSetup`
  (`electron-vite build --mode e2e`, which exposes `window.__store`).

`tests/playwright.config.ts` pins **workers to 1 under `CI`** (two Electron
apps per VM contended enough to flake timing-sensitive terminal specs on the
old upstream runners); parallelism comes from shards, not workers. Tune the
shard count by editing the `matrix.shard` list and the `/4` suffix together.
Failure traces upload as `e2e-traces-shard-N` artifacts.

Local equivalent: `pnpm test:e2e` (no shard arg).

### `e2e-aterm-worker-on` — macos-15 (~25–35 min) — **F16 gate**

`pnpm run test:e2e:aterm-worker-on`. Production ships the shared aterm render
worker **ON** (`window.__atermWorkerRender !== false`), but the default e2e
fixture forces it OFF because most specs assert via main-thread canvas
internals that a transferred OffscreenCanvas breaks. This job runs the curated
worker-compatible specs (clipboard, query replies, a11y, selection, plus the 3
dedicated worker specs) with `ORCA_E2E_ATERM_WORKER=1` — the exact render path
real users get — so worker-path regressions can no longer ship green.

### `windows` — windows-latest (~40 min cold / ~25 min warm)

| Step                        | Local equivalent                                              |
| --------------------------- | ------------------------------------------------------------- |
| Typecheck                   | `pnpm tc`                                                     |
| Addon + node-pty runtime    | `pnpm run build:terminal-addon` + `node config/scripts/ensure-native-runtime.mjs --runtime=node` |
| Daemon/runtime unit lane    | `pnpm exec vitest run --config config/vitest.config.ts src/main/daemon src/main/pty src/main/providers src/main/runtime` |

**Scoping rationale:** Windows is the one platform that ships the **Node
named-pipe daemon** (the Rust daemon's transport is Unix-socket only), so the
highest-value Windows coverage is the daemon + PTY + provider + runtime suites
(~150 test files / ~2,600 tests as of 2026-07; re-count with
`pnpm exec vitest list --filesOnly` on the same paths) where platform behavior
actually diverges — plus a full typecheck. The full vitest suite is not run on
Windows yet purely for runner time; expand the path list in `pr.yml` (or
switch the step to `pnpm test`) once its duration is measured and acceptable.

### Job graph and cost control

`core` and `rust` and `windows` start immediately. The three macOS jobs
(`daemon-parity`, `e2e` shards, `e2e-aterm-worker-on`) declare
`needs: core` — macOS minutes bill at 10× on private repos, so commits that
fail cheap Linux gates never reach the expensive lanes. PR pushes cancel
superseded runs; `main` pushes never cancel (they are the regression
baseline).

## `release-mac.yml`

Manual only (`workflow_dispatch`). Inputs:

- `arches` — comma-separated passthrough to `ORCA_MAC_BUILD_ARCHES`
  (e.g. `arm64` or `x64,arm64`). Empty = runner host arch only
  (`config/electron-builder.config.cjs` default off the release path).

Steps: `pnpm run build:mac` (desktop bundles + terminal addon + Rust daemon +
computer-use helper + electron-builder dmg/zip), then the **arch assertion**,
then artifact upload (`orca-mac-<run id>`, dmg + zip, 14-day retention).

Lane-specific environment:

- **`ORCA_ALLOW_NO_TELEMETRY=1`** — `build:desktop` starts with
  `verify-telemetry-constants.mjs --preflight`, which hard-fails when
  `ORCA_POSTHOG_WRITE_KEY` is unset. This lane builds dark smoke artifacts on
  purpose (the preflight prints the DARK STAGING BUILD warning and passes);
  distributable staging builds must instead inject the write key via the
  `build:mac:release` path.
- **Rust targets** — the toolchain step installs both darwin stds
  (`x86_64-apple-darwin`, `aarch64-apple-darwin`) so any `arches` input works
  regardless of runner host arch; `config/scripts/mac-build-arches.mjs` asserts
  the std for every requested arch before cargo runs.

**Arch assertion:** for every unpacked app electron-builder emits
(`dist/mac` → `x86_64`, `dist/mac-arm64` → `arm64`, `dist/mac-universal` →
both), assert `Contents/Resources/orca-daemon` and
`Contents/Resources/orca_node.node` exist and that `lipo -archs` reports every
arch the app dir claims. The step globs `dist/mac*/*.app` rather than a fixed
app name because `productName` forks by build identity ("Orca Staging" by
default, "Orca" only under `ORCA_PUBLIC_IDENTITY=1` — see
`config/electron-builder.config.cjs`). This catches the "arm64 app shipping an
x64-only daemon" class of packaging bug (audit F2) even though cargo builds
host-arch binaries. The ship-vehicle workstream is also adding a
packaging-time `afterPack` assertion under `config/scripts/`; the workflow
step is the always-on backstop and works whether or not that hook is present.

Signing/notarization is deliberately out of scope: this lane produces
smoke-test artifacts, not distributables. Real releases go through
`build:mac:release` on a machine with signing identities.

## Deviations from audit F15/F16 guidance — open follow-ups

The staging-launch audit's fix guidance asked for more than this pipeline
delivers. These are **conscious scope reductions, not closed items** — each
needs either explicit owner acceptance or a follow-up lane:

- **No gauntlet on PRs** (audit listed it as "minimum before staging"). The
  engine conformance/perf gauntlet is hours-long and owned by the aterm bump
  flow; running it per-PR would dominate runner budget. Follow-up: a scheduled
  (e.g. nightly/weekly) gauntlet lane, or acceptance that it stays manual.
- **No bench JSONs as trend artifacts** (also "minimum before staging").
  Timing numbers off shared runners are noise, but the audit's ask was
  archival for trend analysis, not gating. Follow-up: upload `bench:*` JSON
  outputs as artifacts from a scheduled lane once baselines exist.
- **Release lane is mac-only** (audit: per-platform). Fork ships mac staging
  builds first; `release-linux`/`release-win` mirroring `release-mac.yml` are
  straightforward to add when those platforms ship.
- **Windows lane omits the terminal-perf suites** the audit named — same
  shared-runner-noise rationale as the bench exclusion.
- **F16 residual:** the per-PR `e2e-aterm-worker-on` job gates the shipped
  worker-ON path for the curated 7 specs, but the default 4-shard suite still
  forces the worker OFF for the remaining ~125 specs, and the perf/latency
  e2e suites (which the audit flagged as measuring the non-shipped path) are
  not in CI at all. Inverting the fixture default lives in `tests/**`
  (outside this workstream); until that lands, F16 is mitigated, not closed.

## Deliberate exclusions (and how to run them manually)

| Not in CI                                   | Why                                                                | Run locally with                              |
| ------------------------------------------- | ------------------------------------------------------------------ | --------------------------------------------- |
| `pnpm gauntlet` (engine conformance/perf)   | Engine-repo gate; hours-long, owned by the aterm bump flow          | `pnpm gauntlet`                                |
| Perf benches (`bench:*`, `test:e2e:terminal-perf:*`) | Timing numbers off shared runners are noise, not budgets — needs dedicated hardware + baseline artifacts first | `pnpm bench:startup`, `pnpm run test:e2e:terminal-perf` |
| SSH docker suites (`test:e2e:ssh-*`)        | Need Docker + POSIX ssh orchestration; env-gated (`ORCA_E2E_SSH_DOCKER=1`) and skipped by default anyway | `pnpm run test:e2e:ssh-docker-perf`            |
| Headful e2e project (`@headful` specs)      | Requires a visible window/pointer capture; excluded from the `electron-headless` project CI runs | `pnpm run test:e2e:headful`                    |
| `parity` (wasm renderer parity)             | Covered indirectly by `check:aterm-pin` + committed-artifact drift guard in `lint`; full run needs a wasm toolchain | `pnpm parity`                                  |
| Linux/Windows packaging lanes               | Fork ships mac staging builds first; add `release-linux`/`release-win` mirroring `release-mac.yml` when needed | `pnpm run build:linux` / `pnpm run build:win`  |
| Full vitest on Windows/macOS                | Ubuntu runs the full suite; other OSes run scoped lanes until durations are measured | `pnpm test`                                    |

## Maintenance notes

- **Script-name parity is the contract.** Workflows only call existing
  `package.json` scripts (plus `node`/`cargo`/`vitest` invocations those
  scripts themselves use). If a script is renamed, grep
  `.github/workflows/*.yml` for it.
- Node 24 matches `engines.node`; pnpm version comes from the `packageManager`
  field via `pnpm/action-setup` (no version drift to maintain in workflows).
- Rust toolchain is rustup `stable` (workspace needs ≥ 1.96; the build scripts
  pin cargo/rustc to rustup stable themselves).
- All multi-line steps run under `bash` on every OS (workflow-level
  `defaults.run.shell`), per the repo's cross-platform rule.
