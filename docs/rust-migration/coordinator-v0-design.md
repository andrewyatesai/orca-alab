<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Andrew Yates -->

# Coordinator v0 — technical design

**Status:** design (2026-07-16) · **Plan slot:** moonshot Wave 2 (the Goal-2 product) ·
**Product gates:** the daughter test, [`orca-from-scratch-blueprint.md`](./orca-from-scratch-blueprint.md) §3 Wave 1.
**One sentence:** a thin, calm client of orcad — a grid of terminals-as-agents with a plain-language
attention queue — wearing Orca's design system, built as a NEW surface (not a port of Orca's wiring).

## Non-goals (v0)

No worktree management, no git surfaces, no integrations, no settings pages, no SSH/WSL (local daemon
sessions only), no editor, no mobile. Orca-the-client keeps all of that. v0 is: see every session,
know every agent's state, attach/read/type, resume after any crash, and never lose anything.

## Process model

- **v0 ships inside the orc Electron app as a separate BrowserWindow** (a new `coordinator.html`
  entry in electron.vite.config), NOT a new packaged app. Rationale: reuses the signed shell, the
  design system, and the daemon spawner; zero new distribution surface; and the blueprint's rule is
  about *wiring* (don't inherit the 769-channel surface), not about binaries. The window's renderer
  imports ZERO modules from `src/renderer/src/store` or the IPC-coupled component tree.
- **Transport: the daemon socket, directly.** The coordinator renderer talks to orcad through ONE
  preload-exposed channel pair that tunnels the daemon protocol (main relays socket bytes verbatim —
  no per-feature IPC channels, ever). The TS protocol client already exists and is parity-proven:
  `tools/daemon-parity`'s `DaemonSocketClient` — lift it into `src/shared/daemon-protocol-client.ts`
  (shared, dependency-light) rather than writing a new one.
- **Roles:** control connection for list/attach/metadata; per-session **subscriber** attaches
  (protocol 1019) so the coordinator NEVER steals ownership from Orca panes — both can view the same
  session; resize authority stays with the owner (SIGWINCH lesson). Owner-attach is used only for
  sessions the coordinator itself creates.

## Rendering

- Each tile renders via the existing aterm wasm worker path — reuse
  `src/renderer/src/lib/pane-manager/aterm/` as-is (it is transport-agnostic: bytes in, pixels out).
  v0 may run one engine per visible tile, in-process CPU drawer, and defer the shared-worker
  optimization; the pane manager already supports both.
- Agent state per tile: v0 derives working / needs-me / done / failed from what the daemon already
  knows (foreground process via `process_query`, exit events, OSC 133 exit badges, bell/OSC-9
  notifications). No new detection logic in v0 — read, don't infer.

## UI (the daughter test is the spec)

- **Session grid**: one card per session — big title, live thumbnail or last-lines preview, a
  plain-language status chip (working / needs you / done / failed), time-since-activity. shadcn
  primitives + `main.css` tokens verbatim (~15K-LOC look-port per the blueprint).
- **Attention queue**: a single ordered strip at the top — sessions that need input or finished,
  newest first. Clicking focuses the tile. Nothing else competes for attention.
- **One-click resume**: the grid IS the resume surface — the daemon owns the sessions, so reopening
  the window reattaches everything; there is no "restore" flow to fail.
- **Focused view**: click a tile → full-size terminal with a persistent back-to-grid affordance and
  the status chip. Typing goes through subscriber→owner rules honestly: if the coordinator doesn't
  own the session, v0 shows read-only + a "take over" action (owner attach) rather than pretending.
- **Safety defaults** (the kid constraint): no destructive actions in v0 at all — no kill, no delete;
  stop/retry ships in v0.1 behind a confirm.

## Acceptance gates (measured, per the blueprint)

1. Time-to-first-success: fresh window → reading a live session in < 5 s.
2. Recovery: kill the window mid-session, reopen — every session back, zero bytes lost (daemon
   snapshot + subscriber hydration), < 3 s.
3. Attention correctness: an agent hitting a prompt appears in the queue in < 2 s.
4. The daughter test proper: a non-expert starts Claude Code in a new session, walks away, comes
   back, and knows what happened without help.

## File layout (all new, zero coupling to the legacy wiring)

```
src/renderer/coordinator/          # the new surface (own entry, own html)
  main.tsx / App.tsx               # grid + queue + focused view
  session-tiles.tsx …              # concrete names per content, no utils/
src/shared/daemon-protocol-client.ts  # lifted from tools/daemon-parity
src/main/coordinator-window.ts     # BrowserWindow + the single socket tunnel
```

## Sequencing

1. (landed) subscriber protocol 1019.
2. Lift `DaemonSocketClient` to shared + the main-process socket tunnel (one channel pair).
3. Grid + attention queue reading real daemon state; aterm tile rendering.
4. Focused view + input; acceptance gates as a Playwright spec (`tests/e2e/coordinator-v0.spec.ts`).
5. v0.1: stop/retry with confirm; keep-tail/gap handling for slow subscribers.
