# Orca: ALab Edition Feature Walkthrough

This guide covers the source checkout of
[`andrewyatesai/orca-alab`](https://github.com/andrewyatesai/orca-alab), version
`1.4.147-fork.1`. Install its developer CLI as `orca-dev`; use that name so
commands target this checkout rather than a separate production Orca installation.

This is **Orca: ALab Edition**, an experimental downstream edition of Stably's
Orca. It retains Orca's product workflow while concentrating on the Rust/aterm
terminal stack, native hot paths, recovery behavior, provenance-bound artifacts,
and evidence-driven compatibility. It is still an Electron and React application,
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

The approximately seven-minute visual tour is a guided product lifecycle, not a
feature catalog. Its six chapters contain 14 connected screens:

1. **Terminal first** opens the active workspace terminal or a local scratch
   terminal before any project exists, runs user Quick Commands, reviews
   repository-owned commands before they can run, and distinguishes warm
   process reattachment from layout-and-scrollback restoration after a host
   reboot.
2. **Add a project** chooses the execution host, then opens, clones, or creates a
   codebase without silently changing its branch. It then makes workspace
   creation a separate user action, creates the isolated Git worktree, runs an
   approved shared setup command when configured, and ends at a ready terminal.
3. **Tasks** carries work from GitHub, GitLab, Linear, or Jira into the workspace
   as linked context.
4. **Race approaches** first organizes existing workspaces in Workspace Board
   status lanes, then fans the same task out to isolated Git worktrees for
   Codex, Claude Code, and OpenCode, compares their diffs and checks, keeps a
   winner, and archives the alternatives. Folder-only projects are explicitly
   excluded from the isolation claim because they keep sharing one root; the
   board itself does not launch or merge a race.
5. **Agents & attention** moves from fleet status to an intervention, reply,
   rate-limit/account recovery, and the optional Agents feed. It then searches
   Agent Session History, opens an available local log, jumps to its worktree,
   resumes only on a compatible target, and explains the boundary between
   Manual and full-autonomy modes.
6. **Workbench** uses Quick Open and the Jump Palette to move among workspace
   tabs, files, settings, actions, ports, and rich previews, then attaches
   context to an agent. It also opens the default-on, local-only Floating
   Workspace for cross-repository or scratch terminal, agent, Markdown, and
   browser tabs, and identifies Voice Dictation as an optional focused-pane
   input after model and microphone setup.
7. **Browser & Design Mode** selects rendered UI, packages DOM and computed
   styles plus a source hint and cropped screenshot when available, reviews
   that context, and verifies the hot-reloaded result. Reusing an authenticated
   session is an explicit cookie-import and browser-profile choice.
8. **Review & ship** compares candidates, annotates a revision, pauses for a
   human decision, encounters a failed check or conflict, returns to the same
   workspace to resolve and retry, then requires a human re-review of the
   resolved diff and refreshed checks before staging. It confirms Git and
   PR/MR writes separately and archives the completed workspace.
9. **CLI & Skills** shows an agent discovering version-matched capabilities on
   the host where work runs, operating Orca, and verifying the result.
10. **Orchestration** changes from an independent workspace race to a dependency
    graph with workers, questions, a human-resolved decision gate, coordinator
    relay, recovery, and an accountable coordinator result.
11. **Automations** prechecks a saved workflow, exposes a failed run and its
    history, recovers, reruns, and completes.
12. **Remote & mobile** distinguishes local, SSH, paired-runtime, and
    `orca.yaml` environments; walks through remote work, port forwarding,
    disconnect, and reconnect; and presents Mobile as a paired companion for
    notifications, monitoring, replies, and Quick Commands.
13. **App emulators** separates the Mobile companion from apps under test: a
    workspace-scoped iOS Simulator pane on a local Mac with Xcode, plus
    cross-platform Android/ADB control streamed into Orca's workspace Emulator
    pane, with device discovery, accessibility, logs, actions, visible
    verification, and stale-target recovery. The story retries an explicit
    device, performs a concrete tap/type action, and verifies the resulting app
    state.
14. **Computer Use** checks platform capabilities and permissions before an
    agent inspects a visible desktop app, invokes its advertised Reconnect
    action, and observes the resulting connected state.

Each screen states the user action, the resulting state, and the relevant
boundary or recovery path. Every scripted visual is labelled as an illustrative
example, so the tour does not imply that integrations, checks, accounts, devices,
or host permissions are already configured. The final screen offers both
**Finish setup** and **Return to Orca**. When a visual is taller than the panel,
the measured **More below** affordance pages to the remaining result; compact
windows also offer **Show full description** instead of permanently hiding the
end of a boundary statement.

### Why some capabilities are embedded instead of separate screens

The coverage bar is a distinct product outcome, not a menu item. Workspace
Board, Agent Session History, Floating Workspace, project/user Quick Commands,
and browser-profile reuse materially change how work is planned, resumed, or
executed, so they are explicit beats inside the 14-screen lifecycle.

Voice Dictation is important but is an optional, default-off input method that
requires a speech model and microphone permission; it can type into the same
focused panes already exercised by Terminal and Workbench. It is therefore
embedded in Workbench instead of interrupting the lifecycle with a separate
screen, and Orca's dedicated feature tip still leads users through setup.
Configurable shortcuts are likewise represented through the Jump Palette and
Settings actions rather than treated as their own outcome.

Automatic workspace setup is embedded in **Add a project** because setup runs
when the user later creates a Git workspace, not merely when a repository is
registered. The beat keeps that user action explicit, shows the isolated
worktree and ready-terminal result, and retains the shared `orca.yaml`
command-approval and re-review boundary.

AI-generated commit or PR/MR text, account switching and usage details,
Resource Manager, and workspace cleanup accelerate or maintain the surrounding
review, agent, and workspace flows. They remain available in those product
surfaces and the setup/discovery UI, but do not replace the human confirmation,
host-compatibility, or cleanup boundaries that the walkthrough teaches.

## Terminal engine pin and artifact provenance

The `rust/aterm` submodule is pinned to the public
[aterm](https://github.com/alabsystems/aterm) revision below. The canonical
record of this provenance is the schema-2 artifact manifest at
`src/renderer/src/lib/pane-manager/aterm/aterm_wasm_artifact_pin.json`; the
table restates it as re-verified against the checkout on July 22, 2026. The pin
is a fixed, manifest-bound revision of the public engine, not a live
latest-`main` claim:

| Provenance field                               | Exact value                                                        |
| ---------------------------------------------- | ------------------------------------------------------------------ |
| aterm commit                                   | `e268133cbc6b96add0cddd1fb79e250884035899`                         |
| `git describe --tags --always`                 | `e268133c` (public release snapshot, tag `v0.1.0`)                 |
| Cargo workspace version / embedded WASM marker | `0.1.0` / `aterm(0.1.0)`                                           |
| Artifact manifest                              | schema `2`                                                         |
| Downstream compatibility patch                 | `config/patches/aterm-gpu-wasm-clock.patch`                        |
| Patch SHA-256                                  | `af2e17dda30efbbf3666eeed1ac852aa8dff67d4456f2796bc814209be1bd757` |
| WASM Rust compiler                             | `rustc 1.97.1 (8bab26f4f 2026-07-14)`                              |
| `wasm-bindgen` CLI                             | `0.2.108`                                                          |
| Binaryen optimizer                             | `wasm-opt version 131`                                             |

The pin is the public `aterm v0.1.0` release snapshot at
[alabsystems/aterm](https://github.com/alabsystems/aterm), so a
`--recurse-submodules` clone of this repository resolves the engine from a
public revision. aterm versions as `MAJOR.MINOR.DEV`, where a released snapshot
always carries the public `X.Y.0` form (its internal development version resets
the `DEV` component to `0` at publication).

Schema 2 binds the aterm commit and exact compatibility-patch digest to
all eight generated CPU/GPU files: JavaScript glue, TypeScript declarations,
WASM binaries, and WASM declarations. It records byte length and SHA-256 for
each. The current CPU binary is 3,767,662 bytes with SHA-256
`c48b050ff901eb72f8d4c1a788d6b6959bb8e704519d2cddceaa136c2757dc35`;
the GPU binary is 6,229,686 bytes with SHA-256
`d15eaed0bfecedd8c8d6f17ff53f98a835ecb7da02a5009ecc30f27cb33db558`.
These figures restate `aterm_wasm_artifact_pin.json`; if this document and the
manifest ever disagree, the manifest is the value `pnpm check:aterm-pin`
enforces.

The manifest makes the shipped files auditable and fail-closed, but rebuilding
them byte-for-byte also requires the recorded Rust and Binaryen versions. Orca
pins `wasm-bindgen`; rustup `stable` and the system `wasm-opt` remain explicit
maintainer prerequisites rather than hermetically downloaded tools.

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

The latest pin carries the `v0.58` engine fixes for Codex protected-footer
scrollback, exact and bounded resumable search, renderer recovery and geometry,
and stale keyboard modifier state. Post-tag work adds incomplete-search metadata,
shipping-optimizer benchmark parity, tighter resize fences, fullscreen recovery,
Codex text-release suppression, bounded host-approved OSC 8 schemes,
last-command-output access for WASM hosts, and CPU render scratch reuse. The
repository also contains standalone aterm chrome, settings, effects, and audio
work; those application-only features are not automatically Orca UI features.
Orca consumes the shared engine, renderer, addon, and daemon surfaces.

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

The final full install, lint, typecheck, native-helper, desktop, web,
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

Open **Workspace Board** to group existing workspaces into status lanes and move
cards as priorities change. The status is planning metadata: moving a card does
not start an agent, merge a branch, or publish a result. Create the candidate
workspaces first, then use the board to make their state and ownership legible.

For a Git project, each new Orca workspace is an isolated Git worktree. It gets
its own branch, tabs, terminal panes, browser state, editor state, task link, and
agent sessions, while sharing the repository's object database. This makes it
safe to compare several approaches without agents editing the same checkout.

From the UI, choose **New workspace**, select the project and base branch, and
pick an agent. Orca recognizes Codex, Claude Code, OpenCode, Pi, Grok, and many
other CLIs, and any agent that runs in a terminal can be used directly.

If workspace setup/install commands are configured, Orca runs them in the new
worktree so the terminal is ready for the agent. Shared `orca.yaml`
`scripts.setup` content remains inert until its exact repository command content
is approved; changing that content requires another review. Commands and
duration depend on the repository and on the local or SSH host that owns the
workspace.

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

The right sidebar's **Agents** tab is Agent Session History. Scope it to the
current workspace, project, host, or all recent sessions; search and group the
supported session formats; then inspect a transcript preview. Orca can jump to
the owning worktree, open a real local log read-only when one exists, and resume
a session in its worktree or a new tab only when the transcript contains actual
conversation and the destination host is compatible. Remote or synthetic logs
are not presented as local files, and an unavailable or archived workspace is
reported rather than silently retargeted.

The top-level **Agents** feed is an optional experimental surface. Live terminal
status and right-sidebar history remain useful without enabling it. Manual mode
leaves the selected agent's permission checks in place; full autonomy asks
supported agents to bypass those checks. Neither mode turns a Git worktree into
a machine-security sandbox.

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

Create personal or repository-scoped **Quick Commands** in Settings and launch
them from the tab bar or terminal menu. A project can also commit shared command
or agent-prompt entries in `orca.yaml`. Those repository-owned entries remain
disabled until you review the exact shared command content. Trust covers setup,
default-tab, and project Quick Command command text together, so a content
change requires another review; a matching local command label takes precedence
over the project entry.

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

### Floating Workspace and optional voice input

The default-on **Floating Workspace** supplies local terminal/agent, Markdown,
and browser tabs for scratch work or a directory outside the active project. Its
chosen directory and tabs remain owned by this computer even while an SSH or
paired-runtime workspace is focused; use the project workspace when commands
must execute on that remote host. The floating surface can be hidden or disabled
in Settings without deleting project workspaces.

Optional **Voice Dictation** transcribes into the focused pane in toggle or
hold-to-talk mode. It is disabled by default and requires an installed or
configured speech model plus microphone permission. That makes it a reusable
input method across the workbench rather than a separate project lifecycle.

### Built-in browser

Browser tabs are real Chromium pages alongside the code. **Design Mode** lets
you select an element and send its DOM and computed styles, with a source hint
and cropped screenshot when available, to an agent. Review the generated context
before sending it. To reuse an authenticated session, explicitly choose a
browser profile and import cookies; Orca does not imply that credentials are
captured automatically. The browser is also scriptable through the CLI:

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

AI-generated commit messages and PR/MR descriptions can accelerate drafting,
but they do not combine the write boundaries: confirm the Git commit/push and
the hosted-review publication separately.

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
from the computer. It is separate from the app-under-test emulator workflow
described next.

## 9. Exercise iOS and Android apps

On a Mac with Xcode, open the workspace-scoped **Mobile Emulator** tab or attach
an iOS Simulator through the CLI. The live stream stays in Orca while an agent
inspects accessibility, taps, types, performs gestures, changes permissions,
and verifies the resulting frame. iOS Simulator control runs on the local Mac
that owns Simulator; it is not an SSH-worktree or remote-device promise.

```bash
orca-dev skills get orca-emulator --full
orca-dev emulator list --json
orca-dev emulator attach "iPhone 17 Pro" --focus --json
orca-dev emulator ax --json
orca-dev emulator tap 0.5 0.8 --json
```

Android uses the same `emulator` command namespace with the Android SDK's ADB
backend on macOS, Linux, and Windows. Discover a booted emulator or connected
physical device, retain its exact serial for later calls, and stream that target
into Orca's workspace Emulator pane through scrcpy while the agent installs,
launches, inspects accessibility or logcat, acts, and verifies. An Android
Virtual Device can also keep its own emulator window open; that window is not
the only supported viewing surface.

```bash
orca-dev skills get orca-emulator-android --full
orca-dev emulator devices --json
orca-dev emulator install ./app-debug.apk --reinstall --device emulator-5554 --json
orca-dev emulator launch com.acme.app --device emulator-5554 --json
orca-dev emulator ax --device emulator-5554 --json
orca-dev emulator logcat --lines 100 --device emulator-5554 --json
```

If a target is missing or stale, list devices again, attach or boot it, and retry
with the explicit device ID. Emulator actions affect the running app and its test
data, so inspect the selected target before acting.

## 10. Use Computer Use for desktop apps

Computer Use ships native helpers per platform. It lets an agent inspect visible
desktop apps through accessibility snapshots and operate them with clicks, text
input, key presses, scrolling, dragging, and advertised accessibility actions.
Use the built-in browser CLI for pages inside Orca; use Computer Use for Orca's
own UI, browser windows outside Orca, and other desktop applications.

```bash
orca-dev computer capabilities --json
orca-dev computer permissions --json
orca-dev computer list-apps --json
orca-dev computer get-app-state --app <app-selector> --json
```

On macOS, Computer Use requires Accessibility and Screen Recording permissions;
Linux and Windows do not use that macOS permission flow. `computer permissions`
reports permission state, while `computer capabilities` verifies the available
native helper on every platform. Read the safety and action guidance with:

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

The ALab Edition source build and its aterm pin have been exercised through
independent unit, native, browser, packaging, and live-app paths. The tour,
pin-identity, and artifact-provenance checks below were re-run at the current
checkout on July 22, 2026; older lane counts are explicitly carried forward
rather than relabelled as fresh runs:

- Orca was fast-forwarded to `origin/main` before the final validation; the
  `rust/aterm` submodule is pinned to the public `aterm v0.1.0` release at
  `e268133cbc6b96add0cddd1fb79e250884035899`. The submodule checkout is clean,
  detached at that revision, and matches both the worktree gitlink and the
  manifest's `sourceCommit`.
- Fresh walkthrough validation: all **337/337** focused unit/component tests and
  all **12/12** Electron E2E checks passed. The E2E lane covers all 14 screens,
  standard and compact layouts, reduced motion, keyboard/focus continuity,
  replay persistence, the paired final actions, first-run onboarding handoff,
  and terminal-first startup/restoration. A separate visual audit captured and
  inspected both the first fold and scrolled outcome of every screen.
- The production-like Electron E2E build passed with the pinned CPU/GPU aterm
  artifacts. Full Node, CLI, web, renderer, and mobile typechecking passed.
  Localization verified **10,936** references and parity across all **11,884**
  keys in each shipped locale.
- Rust/TypeScript differential parity passed **1,432** cases across **81** vector
  files and **1,514** assertions, including the expanded 14-screen tour-depth
  protocol and terminal-stream opcode coverage.
- Oxlint, switch exhaustiveness, scrollbar policy, reliability gates, formatting,
  and the max-lines ratchet passed. The repository-wide lint wrapper then stopped
  only at the unrelated append-only skill-history check because upstream's new
  `v1.4.150` tag postdates this fork's committed `v1.4.150-rc.0` snapshot; no
  skill artifacts were regenerated or changed.
- Schema-2 aterm provenance check: all **8/8** generated CPU/GPU artifacts,
  byte lengths, hashes, source commit, and compatibility-patch digest match.
- Current-head upstream focus: the new OSC 8 scheme-capability conformance test
  passed **1/1**, and the aterm WASM library passed **86/86** tests, including
  host authorization and last-command-output coverage.
- Upstream aterm Rust validation: **655/655** passed, comprising **602**
  aterm-effects tests and **53** Codex protected-footer, top-anchored conformance,
  grid scroll-region, and core history/scrollback regressions. Fifteen explicit
  performance benchmarks remained intentionally ignored.
- Later `v0.57`, `v0.58`, and post-release pin advances each regenerated the
  provenance-bound artifacts and passed `pnpm check:aterm-pin`; the fresh
  current-pin evidence is reported above rather than relabelling an older
  engine-suite count as a run against this revision.
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
it does not require a separate `CARGO_HOME` workaround. Regenerating aterm also
requires the stable `wasm32-unknown-unknown` target and Binaryen's `wasm-opt` on
`PATH` (`brew install binaryen` on macOS).

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
worktree, rebuilds the native addon and Rust daemon, refreshes both Cargo locks,
and writes the schema-2 manifest. The subsequent pin check is offline and
fail-closed. Review and stage the submodule pointer, both Cargo locks, patch (if
changed), generated glue/types/WASM, and artifact manifest together.

For active development with rebuilds and hot reload, use:

```bash
cd /path/to/orc
pnpm dev
```

The first run after a terminal-engine change can take several minutes because it
rebuilds the native addon and Rust daemon. Later launches reuse current
artifacts. Rebuild the CLI with `pnpm run build:cli`; that command also refreshes
the `~/.local/bin/orca-dev` symlink when needed.
