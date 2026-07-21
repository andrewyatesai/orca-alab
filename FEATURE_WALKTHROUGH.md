# Orca: ALab Edition Feature Walkthrough

This guide covers the source checkout of
[`andrewyatesai/orc`](https://github.com/andrewyatesai/orc), version
`1.4.144-fork.1`. Install its developer CLI as `orca-dev`; use that name so
commands target this checkout rather than a separate production Orca installation.

This is **Orca: ALab Edition**, an experimental downstream edition of Stably's
Orca. It retains Orca's product workflow while concentrating on the Rust/aterm
terminal stack, native hot paths, recovery behavior, reproducible artifacts, and
evidence-driven compatibility. It is still an Electron and React application,
not a ground-up native rewrite.

## Launch it and confirm readiness

```bash
orca-dev open --json
orca-dev status --json
```

The app is ready when `app.running` is `true` and both `runtime.state` and
`graph.state` are `ready`. Most other CLI commands connect to that running
runtime, so `open` is the right first command after a restart.

### Exact source-build identity

The original **Electron** title came from launching Electron's stock development
app bundle directly. macOS takes the application-menu and Dock identity from that
bundle before Orca's UI starts, so changing only the window title cannot fix it.

`orca-dev open` now prepares a branded development copy of the Electron bundle and
gives it the source build's fixed edition identity. Unless
`ORCA_DEV_DOCK_TITLE` is deliberately overridden, the identity mapping is:

| Surface                      | Exact source-build value                 |
| ---------------------------- | ---------------------------------------- |
| Visible app/menu/window name | `Orca: ALab Edition`                     |
| Managed macOS bundle name    | `Orca: ALab Edition.app`                 |
| macOS bundle identifier      | `com.stablyai.orca.dev`                  |
| Developer profile            | `~/Library/Application Support/orca-dev` |

The wrapper patches `CFBundleName`, `CFBundleDisplayName`, and
`CFBundleIdentifier`, restores Electron framework symlinks, builds and embeds
the notification-status helper when the local toolchain is available, and
attempts to ad-hoc sign the copied app. A helper/signing failure is reported and
falls back cleanly instead of blocking ordinary development. At runtime,
Electron's `app.name`, the native application menu, renderer document title,
and static startup fallback all use **Orca: ALab Edition**. Branch, worktree,
repository-root, and per-checkout identity metadata remain internal for routing,
automation, and collision avoidance; they are no longer used as the visible app
name. The dedicated profile and bundle identifier do not overwrite a production
Orca installation.

If an old process still shows **Electron**, quit that app with **Electron → Quit
Electron** (or `Cmd-Q`) and run `orca-dev open --json` again. The first branded
launch can take a little longer while the managed app bundle is prepared; later
launches reuse it.

Inside Orca, **Help → Explore Orca** opens the visual tour and **Help → Getting
Started with Orca** opens the setup checklist.

## Terminal engine pin and artifact provenance

The `rust/aterm` submodule is pinned to the latest upstream `main` revision as
verified directly against GitHub on July 21, 2026:

| Provenance field                               | Exact value                                                        |
| ---------------------------------------------- | ------------------------------------------------------------------ |
| Upstream commit                                | `49d8fd8a7476e9e49b24650a0269328da9716174`                         |
| `git describe --tags --always`                 | `v0.55-32-g49d8fd8a`                                               |
| Cargo workspace version / embedded WASM marker | `0.55.0` / `aterm(0.55.0)`                                         |
| Artifact manifest                              | schema `2`                                                         |
| Downstream compatibility patch                 | `config/patches/aterm-gpu-wasm-clock.patch`                        |
| Patch SHA-256                                  | `af2e17dda30efbbf3666eeed1ac852aa8dff67d4456f2796bc814209be1bd757` |

The commit is 32 commits after the `v0.55` tag and is represented by aterm's
`[Unreleased]` changelog. Its workspace version is still `0.55.0`; calling it a
tagged `v0.56` release would be inaccurate.

Schema 2 binds the clean upstream commit and exact compatibility-patch digest to
all eight generated CPU/GPU files: JavaScript glue, TypeScript declarations,
WASM binaries, and WASM declarations. It records byte length and SHA-256 for
each. The current CPU binary is 3,752,165 bytes with SHA-256
`d73613ee7899ee96c5cfe5b7de73b32a7a54525d0cb96876238b3eb8fc0e3336`;
the GPU binary is 6,215,431 bytes with SHA-256
`edc79735b4fa8d68502f535d272cfbdc88aa6e85e3cdaa23010e93996e0975dc`.

The small downstream patch changes two GPU present-time measurements from
`std::time::Instant` to the WASM-compatible `web_time::Instant`. The build never
edits the submodule: it creates a detached temporary worktree at the pinned
commit, checks and applies the patch there, builds both renderers, then removes
the worktree. `pnpm check:aterm-pin` fails if the submodule is dirty, the commit
or patch changes, the patch no longer applies, either embedded version marker
drifts, any generated artifact differs in size or hash, or a WASM binary embeds
a local Cargo source path. Rust path-prefix remapping gives generated panic
locations stable virtual roots instead of exposing the build machine's home
directory.

The latest pin carries upstream fixes for Codex protected-footer scrollback,
flooding-TUI presentation freezes, and cursor-trail gaps under load, plus trail
crossfades, the nyan-rainbow aterm default, fresh-ink typing effects, feathered
ribbon ends, per-session matrix rain, and ALab package bundling. Standalone aterm
window chrome and application-only features are not automatically Orca UI
features; Orca consumes the shared engine, renderer, addon, and daemon surfaces.

## Warning cleanup

The warning work is implemented as source/tooling corrections rather than a
blanket quiet mode:

- All 27 prior lint warnings were removed by replacing unsafe `Function` types,
  correcting hook dependencies, and avoiding aliased object fixtures.
- Relay CJS builds rewrite wasm-bindgen's unreachable asynchronous
  `import.meta.url` fallback to an explicit sync-only failure instead of hiding
  esbuild's warning. The relay always initializes its embedded WASM with
  `initSync`.
- The optional aterm scene registry is capability-detected with `Reflect.get`,
  so Rollup no longer treats an intentionally absent export as broken.
- Markdown Custom Highlight selectors live in a small standalone stylesheet,
  bypassing Lightning CSS's unsupported `::highlight(name)` parse path while
  preserving the standards-based browser behavior.
- Generated WASM initialization now uses wasm-bindgen's current
  `{ module_or_path }` form. Deliberately lazy imports pass through deferred
  boundary modules, removing misleading static-plus-dynamic import diagnostics
  without eagerly loading those features.
- Vite's generic 500 kB advisory was replaced by enforced desktop, web, lazy,
  eager-closure, and worker chunk budgets. Oversize regressions now fail with a
  useful policy instead of producing unactionable warnings.
- Cargo runs from the vendored Rust workspace with stock stable flags. Only
  aterm's exact successful temporal-proof line is reclassified as a labelled
  `verified` receipt; every real compiler/build-script warning remains visible.
- The macOS native build uses ScreenCaptureKit instead of the deprecated
  CoreGraphics capture call and works around a mismatched Command Line Tools
  SwiftPM private interface through an isolated, content-addressed ManifestAPI
  cache.
- Vitest is capped at four workers to leave headroom for nested child processes.
  Only Node 24's exact `node:sqlite` `ExperimentalWarning` is filtered; unrelated
  warnings remain visible. Playwright uses the installed local binary and
  normalizes the conflicting `NO_COLOR`/`FORCE_COLOR` child environment.
- The CLI converts its crypto WASM glue for Node/CommonJS, verifies a real
  `--help` execution during the build, and installs `orca-dev` in
  `~/.local/bin` without a privileged `/usr/local/bin` attempt.
- `ORCA_LOCAL_BUILD=1` is the explicit warning-free contributor path for this
  `-fork` version; it compiles telemetry out without presenting a dark-staging
  warning and is rejected in CI/release contexts.

The final latest-pin install, lint, typecheck, native-helper, desktop, web,
relay, CLI, and local production build completed without compiler or bundler
warnings; exact results are recorded under **Validation status** below.

## 1. Add a project

Use **Add Project** to import an existing folder or Git repository, clone a
repository, or configure it on another host. A project is the durable identity;
its host setup records where that project lives on each machine. Adding an
existing repository does not switch its current branch.

Useful discovery commands:

```bash
orca-dev project list --json
orca-dev project setups --json
orca-dev repo list --json
```

`repo` commands remain the convenient Git-repository view, while `project`
commands expose the newer project/host model.

## 2. Create isolated workspaces and start agents

For a Git project, each new Orca workspace is an isolated Git worktree. It gets
its own branch, tabs, terminal panes, browser state, editor state, task link, and
agent sessions, while sharing the repository's object database. This makes it
safe to compare several approaches without agents editing the same checkout.

From the UI, choose **New workspace**, select the project and base branch, and
pick an agent. Orca recognizes Codex, Claude Code, OpenCode, Pi, Grok, and many
other CLIs, and any agent that runs in a terminal can be used directly.

The same flow from the CLI is:

```bash
orca-dev worktree create \
  --repo <repo-selector> \
  --name investigate-login \
  --agent codex \
  --prompt "Investigate the login failure and propose a tested fix" \
  --json
```

Use `--no-parent` for an unrelated top-level workspace. To start a fresh agent
inside the current workspace without creating another worktree:

```bash
orca-dev terminal create --worktree active --command "codex" --focus --json
```

See current work and lineage with:

```bash
orca-dev worktree list --json
orca-dev worktree ps --json
```

## 3. Work across terminal, editor, and browser

### Terminal

The terminal supports tabs, nested splits, WebGL rendering, persistent
scrollback, ordinary shell sessions, and interactive agent CLIs. Orca tracks
agent status so the sidebar, notifications, and mobile companion can show which
sessions are running, waiting, or complete. ALab Edition supplies this through a
shared aterm stack: Rust terminal state/parsing/search/selection, optimized CPU
and GPU WASM renderers, a render worker with explicit fallback paths, a native
Node-API engine for Electron's main process, and a Rust daemon with authenticated
transport, detach/reattach, and recovery. Kitty keyboard protocol handling,
inline images, predictive echo, and terminal effects are carried through that
engine rather than a parallel xterm.js/headless-terminal implementation.

```bash
orca-dev terminal list --worktree active --json
orca-dev terminal read --terminal <handle> --json
orca-dev terminal split --terminal <handle> --direction horizontal --json
orca-dev terminal send --terminal <handle> --text "continue" --enter --json
```

For CLI split commands, `horizontal` means left/right and `vertical` means
top/bottom. Reuse runtime-issued terminal handles for repeated operations.

### Editor and file explorer

The editor provides autosave, syntax-aware editing, changed-file diffs, Markdown
preview and PDF export, image and PDF previews, CSV/TSV tables, and rich Jupyter
notebook viewing/editing. Files and images can be dragged from the explorer into
an agent prompt.

```bash
orca-dev file open README.md --worktree active --json
orca-dev file open-changed --mode diff --worktree active --json
orca-dev file diff src/main/index.ts --worktree active --json
```

### Built-in browser

Browser tabs are real Chromium pages alongside the code. **Design Mode** lets
you select an element and send its DOM, CSS, and cropped screenshot to an agent.
The browser is also scriptable through the CLI:

```bash
orca-dev tab create --url https://example.com --json
orca-dev snapshot --json
orca-dev click --element @e3 --json
orca-dev snapshot --json
```

Element references such as `@e3` come from the latest accessibility snapshot.
Re-snapshot after navigation or a substantial page change. For concurrent
browser work, get a stable `browserPageId` from `tab list --json` and pass it as
`--page <id>` on later commands.

## 4. Review and ship changes

Open **Source Control** to inspect the active workspace's changes as a list or
tree. You can annotate individual diff lines with Markdown review notes and send
the collected feedback back to the agent as one revision request. From the same
workflow, inspect checks and hosted review comments, resolve conflicts, commit,
push, and draft a pull request.

A useful review loop is:

1. Open all changed files in diff mode.
2. Add line comments where behavior or implementation needs revision.
3. Send the review bundle to the active agent.
4. Re-run checks and inspect the updated diff.
5. Commit and publish only after the workspace is cleanly reviewed.

## 5. Turn tasks into workspaces

The **Tasks** surface connects GitHub, GitLab, Linear, and Jira. Enable the
sources you use under **Settings → Tasks**, authenticate their provider, then
browse or search issues, pull/merge requests, reviews, projects, and boards.
Opening a workspace from a task carries the selected item into the workspace as
linked context.

Provider capabilities vary, but Orca can keep common triage and review actions
in-app. Its Linear integration also supports ticket-aware agent workflows such
as reading full context, updating fields, commenting, attaching a PR/MR, and
creating parented follow-up issues. Read the installed, version-matched guide
before scripting writes:

```bash
orca-dev skills get orca-linear --full
```

## 6. Coordinate multiple agents

Use a simple worktree or terminal handoff when one agent can own the whole task.
Use **Orchestration** when a coordinator must supervise several workers and
collect their results. The orchestration model includes task DAGs and
dependencies, dispatches, threaded inter-agent messages, blocking questions,
decision gates, heartbeats, and coordinator runs.

Set it up from **Settings → Orchestration**, then read the guide bundled with
this exact CLI build:

```bash
orca-dev skills get orchestration --full
orca-dev orchestration task-list --json
```

The guide explains the expected task lifecycle and when to use `send`, `check`,
`reply`, `dispatch`, gates, and coordinator runs. This is preferable to inventing
an ad-hoc message protocol between terminals.

## 7. Automate recurring work

An automation saves a prompt and execution target, then runs it manually or on a
schedule. Orca supports common schedule presets, RRULE-backed recurrence, custom
five-field cron expressions, optional precheck commands, missed-run grace
windows, and run history.

Choose whether each run should create a fresh workspace from a branch or use an
existing workspace. Existing-workspace automations can start a fresh agent
session or reuse a live one. The Automations UI also presents supported external
automation managers such as Hermes and OpenClaw when they are installed.

```bash
orca-dev automations list --json
orca-dev automations runs --json
orca-dev automations show <id> --json
orca-dev automations run <id> --json
```

Use `orca-dev automations create --help` for the current creation flags rather
than copying a schedule from another Orca version.

## 8. Work remotely and from mobile

Orca supports two complementary remote models:

- **SSH project setups and worktrees** run terminals, agents, Git, and file
  operations on an SSH host, with reconnect and port-forward support.
- **Remote Orca runtimes** run `orca serve` on another machine and connect from
  the desktop through a pairing code. Saved environments make those runtime
  connections reusable.

```bash
orca-dev serve --project-root <absolute-path> --json
orca-dev environment add --name <name> --pairing-code <code> --json
orca-dev environment list --json
```

For disposable cloud or VM environments, Orca also supports per-workspace
recipes in `orca.yaml`; validate their static wiring with
`orca-dev vm recipe doctor` before provisioning anything.

The iOS and Android **Orca mobile companion** can pair with the desktop runtime
to monitor agents, receive completion notifications, and send follow-ups away
from the computer. This is separate from Orca's emulator panes: the iOS
Simulator and Android/ADB integrations let an agent inspect and operate an app
under development, including taps, gestures, typing, permissions,
accessibility, installation/launch, and logs.

Discover the version-matched emulator guides with:

```bash
orca-dev skills get orca-emulator --full
orca-dev skills get orca-emulator-android --full
```

## 9. Use Computer Use for desktop apps

Computer Use lets an agent inspect visible macOS apps through accessibility
snapshots and operate them with clicks, text input, key presses, scrolling,
dragging, and advertised accessibility actions. Use the built-in browser CLI for
pages inside Orca; use Computer Use for Orca's own UI, browser windows outside
Orca, and other desktop applications.

```bash
orca-dev computer capabilities --json
orca-dev computer permissions --json
orca-dev computer list-apps --json
orca-dev computer get-app-state --app <app-selector> --json
```

macOS may require Accessibility and Screen Recording permission before every
capability is available. `computer permissions` reports the current state and
can open the relevant System Settings pages. Read the safety and action guidance
with:

```bash
orca-dev skills get computer-use --full
```

## CLI discovery

The CLI is deliberately self-describing:

```bash
orca-dev --help
orca-dev agent-context --json
orca-dev skills list --json
```

`agent-context` returns the machine-readable command schema. `skills list`
shows the guides bundled with this build, so an agent can follow contracts that
match the installed version instead of relying on stale global documentation.

## Validation status

The latest aterm pin and final ALab Edition source build have been exercised
through independent unit, native, browser, packaging, and live-app paths:

- Fresh `HEAD == origin/main` verification for Orca and aterm, with aterm at
  `49d8fd8a7476e9e49b24650a0269328da9716174` and its submodule checkout clean.
- Schema-2 aterm provenance check: all **8/8** generated CPU/GPU artifacts,
  byte lengths, hashes, source commit, and compatibility-patch digest match.
- Latest upstream aterm Rust validation: **655/655** passed, comprising **602**
  aterm-effects tests and **53** Codex protected-footer, top-anchored conformance,
  grid scroll-region, and core history/scrollback regressions. Fifteen explicit
  performance benchmarks remained intentionally ignored.
- Scroll-intent integration unit set: **156/156** passed. The non-vacuous
  rendering golden passed **2/2**, including emoji-table alignment and the
  worktree-switch scroll restoration that exposed the resume race.
- Worker-enabled Electron lane: **14 passed, 1 intentional in-process-only
  skip**. This covered GPU rendering on a worker-owned `OffscreenCanvas`, CPU
  fallback contracts, selection, search, PTY query replies, clipboard,
  accessibility, OSC colors, spill/chrome, and TUI wheel reporting.
- Native live aterm verifier: cwd, title, alternate-screen, mouse mode,
  scrollback, history recovery, snapshot, and rehydration checks all passed.
- Exact identity tests: **22/22** passed. Live macOS inspection found
  `CFBundleName` and `CFBundleDisplayName` set to **Orca: ALab Edition**, bundle
  identifier `com.stablyai.orca.dev`, a valid deep code signature, application
  menu/window title **Orca: ALab Edition**, menu item **About Orca: ALab
  Edition**, and the same name inside the native About dialog.
- `orca-dev status --json` reported the app running with both runtime and graph
  state `ready`. A live Computer Use snapshot independently reported app name
  and window title **Orca: ALab Edition**.
- Frozen dependency install, lint, typecheck, formatting, desktop/web/relay/CLI
  builds, macOS native helpers, addon build/cache validation, and the complete
  `ORCA_LOCAL_BUILD=1 pnpm build` all passed. The final build emitted no
  compiler or bundler warnings; the temporal Cargo proof receipt was labelled
  `verified`, not hidden.

Earlier broad regression evidence also remains green: Vitest **31,894** tests,
renderer checks **379**, differential/parity **1,513**, and terminal gauntlet
**109**. Those larger historical counts complement the fresh latest-pin lanes
above; they are not relabelled as a newly repeated full-suite run.

## Update and maintain the source build

Prerequisites are Node.js 24, pnpm, and a rustup-managed stable Rust toolchain
version 1.96 or newer. The checkout vendors its Rust crates and WASM artifacts;
it does not require a separate `CARGO_HOME` workaround.

To update and rebuild:

```bash
cd /path/to/orc
git pull --ff-only
git submodule update --init --recursive
pnpm install --frozen-lockfile
pnpm run build:rust-daemon
ORCA_LOCAL_BUILD=1 pnpm build
orca-dev open --json
orca-dev status --json
```

`ORCA_LOCAL_BUILD=1 pnpm build` is the warning-free contributor build path for
this `-fork` version. It deliberately leaves telemetry constants unset and
compiles telemetry transport out; it is rejected for CI and release builds and
must not be used to produce a shippable staging artifact.

Maintainers advance and regenerate the terminal engine as one provenance-bound
operation:

```bash
pnpm bump:aterm
pnpm check:aterm-pin
```

`bump:aterm` fetches and detaches at the requested/latest upstream revision,
rebuilds CPU and GPU artifacts through the isolated compatibility-patch
worktree, and writes the schema-2 manifest. The subsequent pin check is offline
and fail-closed. Review and stage the submodule pointer, patch (if changed),
generated glue/types/WASM, and artifact manifest together.

For active development with rebuilds and hot reload, use:

```bash
cd /path/to/orc
pnpm dev
```

The first run after a terminal-engine change can take several minutes because it
rebuilds the native addon and Rust daemon. Later launches reuse current
artifacts. Rebuild the CLI with `pnpm run build:cli`; that command also refreshes
the `~/.local/bin/orca-dev` symlink when needed.
