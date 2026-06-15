# Orca → Native Rust: Target Architecture

> Status: living document. Per-subsystem detail, the dependency→crate map, and
> the ordered phase plan are generated into `functional-map.md`,
> `dependency-map.md`, and `migration-plan.md` in this folder.

## Goal

Convert Orca from an Electron + React + TypeScript application (~908K LOC) into a
**fully native** application with a **cross-platform Rust core** and **thin,
platform-specific native wrappers**. All third-party dependencies are
**vendored** and **stripped** to the functionality Orca actually uses.

## Confirmed shape (from the owner)

- **Core = Rust, built and verified with _Trust_** ("trusted Rust"). Trust
  (`~/trust`, `github.com/andrewyatesai/trust`) is a verification-oriented fork
  of `rust-lang/rust`: `trustc` compiles Rust *and* proves properties about it
  (panic-safety, integer overflow, out-of-bounds, div-by-zero, ownership
  invariants, contract postconditions) via a MIR verification pass, driven by
  `tcargo trust check`. Goal: **extreme performance + machine-checked safety**
  on the shared core.
- **Native experience per platform via a thin wrapper, in any language.** The
  cross-platform requirement is satisfied by the Rust core; the wrapper is
  platform-specific and need not be Rust. macOS → **SwiftUI** for a true native
  feel; Linux/Windows → their own thin native shells over the same core.
- **Terminal = `aterm`** (`~/aterm`, `github.com/andrewyatesai/aterm`), the
  owner's terminal project — we embed its headless engine. ✅ `aterm` is now a
  full ~45-crate engine (parser, grid, tiered scrollback, SGR/colour model,
  OSC-7, mouse modes, search/selection, shell integration), differential-tested
  against Alacritty. `orca-terminal` is wired to it as a thin adapter over
  `aterm-core::Terminal` (branch `aterm-integration`): the `HeadlessTerminal`
  surface, `orca-ffi` C ABI, and `orca-session` are unchanged — only the engine
  underneath the adapter changed from the `vte` subset to aterm. The build is
  fully offline and self-contained: aterm's source is vendored in-repo at
  `third_party/aterm` (its own workspace, outside `rust/`; only the
  `aterm-core` closure compiles) and its crates.io dep-closure is vendored
  under `rust/vendor`. A later cleanup can swap the in-repo copy for an aterm
  git-rev pin once that repo is pushed.

## Layering

```
┌─────────────────────────────────────────────────────────────────────┐
│  Platform wrappers (thin, native, any language)                      │
│   • macOS:   SwiftUI app — window/menus/dock, notifications,         │
│              signing/notarization, sandbox, Sparkle update;          │
│              renders the native UI; calls the Rust core over FFI     │
│   • Linux:   GTK / winit shell over the same core                    │
│   • Windows: WinUI / Win32 shell over the same core                  │
├─────────────────────────────────────────────────────────────────────┤
│  Rust core  (cross-platform, compiled + VERIFIED with Trust)         │
│                                                                       │
│  orca-ffi          stable C ABI / UniFFI surface the wrappers call   │
│  orca-terminal     headless VT engine (aterm) — grid, parser, modes  │
│  orca-runtime      workspace/session orchestration, RPC, remote      │
│  orca-agents       provider adapters (claude/codex/gemini/…)         │
│  orca-scm          provider-neutral source control + review          │
│  orca-git          git ops, worktrees, diff, blame                   │
│  orca-pty          local PTY (replaces node-pty) + process mgmt      │
│  orca-ssh          SSH remote runtime (replaces ssh2)                │
│  orca-relay        pairing + mobile/remote transport (replaces ws)   │
│  orca-store        persistence (sqlite), settings, caches            │
│  orca-net          HTTP clients, proxy, rate limiting                │
│  orca-speech       STT (sherpa-onnx via FFI) + dictation             │
│  orca-telemetry    event model + gated transport                     │
│  orca-core         PURE logic ported from src/shared (no IO) ✅      │
└─────────────────────────────────────────────────────────────────────┘
```

`orca-core` exists today, test-verified — see "Proof of pipeline". Crate names
are provisional; the generated domain grouping reconciles them with the real
subsystem map.

## Trust (trusted Rust) integration

- The core is **ordinary Rust** so it builds with stock `cargo` everywhere
  (CI, contributors), AND is **verifier-ready** for Trust: `trustc` is
  Rust-compatible by default, so the same sources compile under the Trust
  sysroot with no source changes.
- **Verification lane:** `tcargo trust check --format json` over the core proves
  the safety obligations Trust generates. The pure-logic crates (`orca-core`:
  path resolution, C-quote decoding, id parsing) are the ideal first verified
  target — they are exactly overflow/bounds/panic-shaped code.
- **Code is written verifier-friendly:** `#![forbid(unsafe_code)]` on core
  crates; panic-free style (no `unwrap`/indexing without a proven guard) so
  panic-safety obligations discharge. Example: `cross_platform_path`'s `..`
  handling was rewritten to `is_some_and` specifically so the empty-stack case
  is provably non-panicking.
- **Status:** the Trust stage2 sysroot is **not built locally yet**
  (`~/trust/build/<host>/stage2/bin/` is absent). Building it compiles a full
  rustc fork from source (long). It is gated behind an explicit owner decision;
  until then the core builds/tests on stock Rust and the verification lane is
  documented but not yet wired into CI.

## Terminal strategy — `aterm`

The current design keeps a **server-side terminal** so sessions survive
reconnect, SSH, and remote/mobile replay:
`src/main/daemon/headless-emulator.ts` wraps `@xterm/headless` +
`@xterm/addon-serialize` to maintain a grid, track cwd via **OSC-7**, track
mouse modes via **DECSET**, and emit serializable **snapshots**. The native
replacement must reproduce all of that headlessly, and additionally drive the
on-screen terminal so headless (daemon/SSH) and rendered terminals share one VT
implementation.

This engine lives in **`aterm`**. Because `aterm` is currently empty, there are
two ways to seed it (owner decision pending):

1. **Author fresh in `aterm`** — a purpose-built Rust VT engine (`vte` parser +
   our own grid/scrollback/mode model + snapshot), maximal control and the
   cleanest fit for the snapshot/remote-replay needs above.
2. **Seed from a vendored library engine** — start from `alacritty_terminal`
   (Alacritty's reusable `Term`/grid + `vte`) or `wezterm-term`, vendored under
   `aterm` and stripped to grid+parser+modes, then adapt. Fastest to parity;
   precedent: Zed's GPUI terminal embeds `alacritty_terminal`.

Either way the engine is consumed by `orca-terminal` (headless API for the
daemon/SSH path) and rendered by each platform wrapper (the macOS SwiftUI app
draws the grid with CoreText/Metal; other shells with their native stack).

| Current (`@xterm/headless`) | Native (`aterm`)                          |
| --------------------------- | ----------------------------------------- |
| `Terminal` grid + scrollback| grid + history model                      |
| ANSI/DECSET parsing         | `vte` parser                              |
| `SerializeAddon` snapshot   | grid walk → compact snapshot              |
| OSC-7 cwd tracking          | parser cwd hook                           |
| mouse mode tracking         | term-mode flags (`MOUSE_*`, `SGR_MOUSE`)  |

## UI strategy — native per platform over a Rust core

"Native OSX experience" with "any language for the wrapper" resolves the earlier
open question: the UI is **native per platform** (not one Rust-drawn UI), and
the wrapper language is free. macOS uses **SwiftUI** for the best Apple feel;
Linux/Windows get their own thin shells. All of them are thin: they render UI
and forward intents, while every behavior, state transition, and service call
lives in the Rust core behind a stable **`orca-ffi`** surface (C ABI or UniFFI).
The existing `native/computer-use-macos` Swift package is the precedent for
Swift↔native interop already in this tree. This is the Ghostty model (shared
non-UI core + native SwiftUI macOS / GTK Linux apps), with Rust+Trust as the
core.

## Dependency vendoring & stripping policy

**Status: live.** `rust/vendor/` holds all third-party crates (today: `regex`
+ `regex-automata`, `regex-syntax`, `aho-corasick`, `memchr`), pinned by
`rust/Cargo.lock`, with `rust/.cargo/config.toml` redirecting crates.io →
`vendor/` and `[net] offline = true`. `orca-text` builds offline from them.

1. `cargo vendor` populates `rust/vendor/`; `.cargo/config.toml` redirects
   crates.io to it so builds are offline and reproducible. _(done)_
2. Each vendored crate is **stripped** via minimal feature sets:
   `default-features = false` + only the features in use (e.g. `regex` keeps
   `std, perf, unicode-case, unicode-perl`; other unicode tables compile out).
   Physical source pruning is a later refinement where a crate is forked.
3. Native C/C++ libs we cannot rewrite (**sqlite**, **onnxruntime** for sherpa)
   are vendored as source and built via `cc`/`bindgen` behind a thin safe
   wrapper crate, not linked from the system.
4. The full dep→crate table is in `dependency-map.md`.

## Migration phasing (high level)

Leaf-first, lowest-risk-highest-leverage first. Each phase ports behaviour
**with the original test cases translated** so fidelity is verifiable, and —
where it is pure-logic — runs through the Trust verifier.

1. **Pure core** (`orca-core`) — `src/shared/*` pure logic. _In progress._
2. **IO services** — `orca-git`, `orca-pty`, `orca-store`, `orca-net`,
   `orca-ssh`: deterministic, headless, integration-testable without a UI.
3. **Domain services** — `orca-scm`, `orca-agents`, `orca-runtime`,
   `orca-relay`, `orca-telemetry`.
4. **Terminal** — `aterm` engine + `orca-terminal`, headless first (daemon/SSH
   parity), then rendered.
5. **FFI surface** — `orca-ffi` stabilized for the wrappers.
6. **Wrappers** — thin SwiftUI (macOS) first, then Linux/Windows; cut over from
   Electron and delete the TS tree per-subsystem as parity lands.

The detailed, dependency-ordered plan is in `migration-plan.md`.

## Proof of pipeline (done)

Fifteen crates are ported and green offline (`cargo test`: **738 passed**,
clippy clean), each module carrying its **original test cases translated
verbatim** — so `cargo test` is the behavioural-parity gate:

- **`orca-core`** (47 modules) — `src/shared` pure logic, zero-dependency,
  `#![forbid(unsafe_code)]`, panic-free (Trust-ready).
- **`orca-text`** (6) — `git_remote_error`, `mcp_env` (env masking),
  `pi_agent_kind`, `skill_metadata` (YAML frontmatter), `agent_tab_title`
  (prompt → tab title), and `workspace_name` (slug + intent-name derivation),
  offline from vendored `regex`.
- **`orca-git`** (10) — full remote-ops subsystem (push/pull/fetch/fast-forward/
  rebase-from-base + upstream status + branch rename + check-ignore) over a
  `GitRunner` boundary (native `std::process` shim).
- **`orca-store`** (1) — SQLite adapter on **vendored, bundled SQLite**
  (C amalgamation compiled offline via `cc`).
- **`orca-pty`** (1) — native PTY spawning (replaces `node-pty`) on **vendored
  `portable-pty`**; tests spawn a real PTY child.
- **`orca-session`** (1) — live session: PTY output streamed into the headless
  terminal on a background thread (composes `orca-pty` + `orca-terminal`).
- **`orca-terminal`** (2) — headless VT engine (replaces `@xterm/headless`) on
  **vendored `vte`**: grid/cursor/scroll + OSC-7 cwd + snapshot/resize; plus the
  color-scheme (DEC mode 2031) subscribe/unsubscribe scanner.
- **`orca-runtime`** (1) — multi-agent orchestration store (messages/tasks) on
  `orca-store`'s vendored SQLite, full schema from `orchestration/db.ts`.
- **`orca-config`** (7) — project/config inspection on **vendored `serde_json`**:
  MCP config inspection, package-manager setup suggestion (injected file-read),
  repo-icon sanitize/build, Pi-overlay settings merge, project-group
  normalization, workspace-status normalization (+legacy-default migrations),
  and feature-interaction catalog + persisted-state normalization.
- **`orca-net`** (1) — network tier: proxy URL normalize/redact, env precedence,
  and child-process proxy-env construction. std-only, zero-dependency.
- **`orca-crypto`** (1) — NaCl `box` (X25519 + XSalsa20-Poly1305) E2EE for the
  remote-runtime transport, on **vendored `crypto_box`**; verified
  **wire-compatible with `tweetnacl`** against the canonical NaCl `box` vector.
- **`orca-agents`** (8) — agent-CLI domain, commit-message generation ported
  **end-to-end**: the 8-agent spec table + lookups, prompt assembly + draft
  prompt/response splitting, **plan** (agent+prompt → spawn-ready binary/argv/
  stdin), custom-command tokenize/plan, agent-output/error cleanup, model-
  discovery parsers, pull-request field generation, over **vendored `regex` +
  `serde_json`**, TUI-agent auto-pick / enable-disable filtering, and
  **agent-status payload** parse/normalize (state allow-list + UTF-16-safe
  surrogate-preserving field caps).
- **`orca-relay`** (3) — terminal **binary-stream framing** (fixed 16-byte LE
  header + multiplexed text/JSON payloads), the **pairing handshake**
  (`orca://pair` deep link), and the **E2EE channel** state machine (NaCl-box
  handshake → transparent encrypt/decrypt, a pure reducer with transport/timer/
  RNG injected) over **`orca-crypto`** — replacing the `ws` relay; JSON over
  **vendored `serde_json`**.
- **`orca-ffi`** (1) — the **C ABI** the native wrappers link
  (`liborca_ffi.a`/`.dylib` + `orca.h`); first surface = the headless terminal.

Per-module pattern for the whole migration: read TS + tests → port logic +
tests → `cargo test`/clippy green → (pure logic) `tcargo trust check` → mark the
subsystem ported in `ported-modules.md`. Vendoring is live and proven for all
three dependency modes: pure-Rust (`regex`, `serde_json`, and the `crypto_box`
X25519/XSalsa20-Poly1305 stack — 20 crates incl. `curve25519-dalek` +
`fiat-crypto`, built offline), native-C-via-`cc` (`sqlite`), and native-syscall
(`portable-pty`).

The **full vertical slice runs**, including a *live* terminal:
`native/orca-macos` links `liborca_ffi` via `orca.h`; `swift run orca-smoke`
spawns a real shell command in a PTY through `OrcaKit.OrcaSession` and reads its
streamed output back in Swift, and `OrcaUI.SessionTerminalView` (SwiftUI)
renders that grid. So every layer — native Swift/SwiftUI shell → C ABI →
`orca-session` → PTY + `orca-terminal` → vendored `vte` — is demonstrated
working end-to-end, not just planned.

## Verifying parity — `tools/parity` + `orca-parity`

Because Orca is being **re-implemented fresh** (TS as the reference spec, not a
byte-for-byte transliteration), correctness is checked **differentially**: the
[`tools/parity`](../../tools/parity/README.md) harness runs the fresh Rust port
and the original TypeScript over **one shared JSON vector corpus** and diffs
their outputs. Both compared values are computed live (Rust via the `orca-parity`
bin; TS via importing the real `src/shared/*.ts`), so an agreement is real
evidence — not same-author test circularity — and a disagreement is a concrete
case to triage. It is a *divergence report*, not a strict gate: intended
fresh-reimplementation differences are marked `allowDivergence` and reported, not
failed.

Each ported module contributes three files (Rust dispatch arm, TS dispatch arm,
golden vectors seeded from its `.test.ts`). The Rust leg additionally self-checks
each output against the transcribed golden and runs with **only the Rust
toolchain** (no Node), so it is verifiable in a headless/offline environment; the
vitest leg (`parity.test.ts`) closes the TS side on a machine with Node.

**Coverage:** 81 of the ported logic modules have parity adapters —
**1043 vectors, 1041 golden checks green offline** (`cargo run -p orca-parity`).
The other 17 are honestly out of differential scope and logged: io-edge functions
that only run via injected fs/exec/clock/socket closures (`git::remote`,
`git::status`, `git::branch_cleanup`, `relay::e2ee_channel`, …), modules sourced
in `src/main` rather than `src/shared`, and pure helpers that TS keeps
non-exported (so there is no TS symbol to feed identical input to).
