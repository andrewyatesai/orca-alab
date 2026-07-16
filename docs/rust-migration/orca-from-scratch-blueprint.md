<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Andrew Yates -->

# Orca From Scratch — same look, structurally better

**Date:** 2026-07-15 · **Grounding:** 3-agent recon (product-surface inventory, main-process service
census, architectural regret list; extracts in the session scratchpad `moonshot/wf4-recon.json`).
Companion to [`extreme-performance-moonshot.md`](./extreme-performance-moonshot.md) (the how) and the
purpose brief (the why). Governing goals: (1) reputation for making a startup's existing product
dramatically better — wins must be adoptable; (2) the household coordinator — many terminals-as-agents,
one calm surface.

---

## 1. The three-number story

**"Looks about the same" costs < 7K LOC.** The entire visual identity is: 253 CSS design tokens
(`src/renderer/src/assets/main.css`, 3,470 lines), a 314-line STYLEGUIDE.md, ~28 shadcn primitives
(2,935 LOC), plus layout shells (tabs, status bar, palettes ≈ another few K). Pure CSS/React on the
token layer, zero transport coupling — **ports verbatim**. Preserving Orca's look is the cheapest part
of the entire program.

**The product is ~906K non-test LOC** (renderer 551,677 · main 293,949 · shared 60,070; plus a 95K-LOC
React Native mobile app). But the coupling is concentrated: only ~40% of the renderer (345 files,
220K LOC) touches `window.api` at all; 146K LOC of components are *extracted pure logic modules*, and
renderer tests are another 382K LOC — 41% of renderer lines — which port as **executable specs** (and
are exactly the factory's parity corpus).

**The rot is architectural and enumerable, not diffuse:**
- **Three parallel protocol surfaces**: 616 renderer invoke channels + 182 event channels (769 total,
  670 ipcMain registrations across 277 files), an 88-method mobile E2EE RPC re-implementing the app API
  (11,756 LOC), and the daemon protocol. One verified protocol replaces all three.
- **One god object**: `runtime/orca-runtime.ts`, 27,463 lines (workspace/agent/terminal/mobile
  authority; 549 "Why:" comments).
- **A ~3,900-LOC delivery-reliability shim** — 27% of `ipc/pty.ts` (char-ACK window, resync probes,
  write-off heal lane, dispatcher handshake + 10s watchdog, hidden-delivery gates, flood-suppression
  backstops) plus ~1,250 renderer LOC and an 11,761-line pty.test.ts — all existing because PTY bytes
  are pushed over `webContents.send` with no delivery guarantee and a receiver that dies independently.
  The compensators even fight each other (the rc.7.perf restore-feedback loop needed a third mechanism
  to police the first two).
- **Duplicate state everywhere**: up to 3 emulators parse every byte; a three-way snapshot-authority
  arbitration (provider vs headless vs renderer); a TS daemon twin (~4,364 LOC) kept only because the
  Rust daemon lacks Windows transport; a 1,124-line xterm facade for an engine no longer in
  package.json; 6 independent gate sites for the SIGWINCH/placeholder-grid class.

## 2. The architecture (what "better" means structurally)

1. **orcad owns all state**: PTYs, sessions, git (gix + orca-git), fs-watch, agent status, workspace
   model, persistence. **One emulator per session** — the daemon's. No mirrors, no snapshot
   arbitration, no replay-into-fresh-emulator (the renderer's 274-line replay-guard becomes moot).
2. **One verified protocol, three transports**: credit/seq flow control (the moonshot's TLA+/ty-checked
   transport) over local socket, over SSH (deleting the Node relay's bespoke framing), and to mobile
   (deleting the 88-method RPC duplication). Views *subscribe and resume from seq* — a hidden pane is
   just an unsubscribed view, so the entire ACK/heal/resync/watchdog/drop-marker stack is
   **impossible to need**, not merely rewritten.
3. **Clients are projections.** Resize authority is explicit in the protocol (subscriber ≠ owner), so
   the SIGWINCH gate class dies. The renderer keeps: design system, shells, pure logic modules, aterm
   worker rendering. It loses: 769 ad-hoc channels (replaced by one typed protocol client), its share
   of the reliability shim, and every "who is the authority" reconciliation.
4. **The factory feeds it**: hot modules port with parity from the 382K-LOC test corpus; the god object
   is not ported — its domains (workspace/agent/terminal state) become daemon domains one at a time.

**Main-process disposition** (of 294K non-test LOC, from the census): ~55% moves daemon-side
(git/worktrees 24K, agent-provider ecosystem 53K, hosted-git providers 28K, SSH 14K, fs 11K,
runtime state core 20K, persistence 8K, …), ~15% stays client (browser/CDP, windows/menu/tray,
updater, emulator streaming, speech), **~20% deletes by construction** (~58K: mobile RPC + relay,
TS daemon twin + degraded fallbacks, the pty shuttle/flow-control/hidden-gate layer, NAPI marshaling),
~10% splits (telemetry, profiles, observability).

## 3. The strategy: Theseus, not big-bang

Do **not** rebuild Orca as a project. The from-scratch Orca is what the coordinator *grows into*,
because every enabling piece is already on the roadmap and the expensive-looking parts are cheap:

1. **Now**: coordinator v0 = orcad attach → session grid → agent status → notifications. Port the
   design system + ui/ primitives + tab/status shells verbatim (the "looks like Orca" contract,
   < 7K LOC). Each pane is an aterm view over the subscriber protocol.
2. **Next surface by value**: source control / checks panels (the highest-value daemon-protocol
   consumers — 49K LOC today, re-grown smaller against typed git APIs), then terminal-adjacent chrome
   (quick-open, palettes port near-verbatim), then settings pages against a daemon config API.
3. **Re-grow, never port, the wiring**: the 345 api-touching files are rebound to protocol APIs as
   their surfaces migrate; pure logic modules + tests come across unchanged (and factory-port to Rust
   where hot).
4. **orc-the-fork keeps running** throughout as daily driver, factory specimen, and the before/after
   case study. Mobile stays on the legacy client until the protocol's mobile transport lands.
5. **Flip** when the coordinator is daily-drivable; legacy surfaces that never earned migration
   (emulator pane? feature-wall?) simply never cross — the Move-3 "migrate by value" rule.

**Success criteria for "better", measurable**: same visual identity (token-diff = zero); startup
< 300ms to first attached pane (no 769-channel wiring at boot, codeCache + snapshot); one parse per
byte; zero delivery-reliability code (the property is the protocol's theorem); ~40–50% less code for
the same daily-driven surface area; every regret-list class structurally impossible, verified by
grepping for its mechanisms and finding nothing.

## 4. Goal alignment

- **Goal 1 (reputation)**: the regret list *is* the case-study material — "adopt the daemon + protocol
  and delete ~3,900 LOC of reliability shims plus an 11.7K-line test file; here are the theorems that
  replace them." Upstream-adoptable increments: libaterm, the byte plane, parse-once, the protocol as a
  library. The blueprint proves the offer generalizes: concentrated-coupling audits like §1 are
  repeatable on any startup's Electron app.
- **Goal 2 (household layer)**: the coordinator starts as the smallest possible client of the same
  stack — the from-scratch rebuild and the family app are the *same artifact* at different ages.
