# The Three-Move Path to Rust-Native Orca

> Status: strategic direction. This doc records the *sequencing* for pulling Orca
> off the Electron/canvas seam and onto a Rust-native core, grounded in crates
> that already exist. The target end-state and per-subsystem detail live in
> [`architecture.md`](./architecture.md); the ordered phase plan lives in
> [`migration-plan.md`](./migration-plan.md). This doc is the "which move first,
> and why" layer above those.

## Why this keeps biting

The current terminal bugs — DPR/pixelation, transferred-canvas quirks,
structured-clone cost — are not engine bugs. They are **seam tax**: the price of
rendering a terminal *through a browser*.

The web canvas path carries **three coordinate systems** (CSS px × device px ×
DPR) and **four process/boundary hops**:

```
PTY → daemon → main → renderer → wasm → canvas → compositor
```

aterm-native has **one** surface: winit hands wgpu physical pixels, the swapchain
presents, done. Every DPR bug and transferred-canvas quirk we've fought traces
back to the seam, not to `aterm-core`. The engine has been innocent almost every
time. So the strategic goal is not "fix the seam better" — it is **remove the
seam** for the surfaces where it hurts most (the terminal), while keeping the web
toolchain where it is genuinely unbeatable (the editor).

## The through-line

This is **not a rewrite** — it is a **reunification**. The migration machinery is
already built:

- the shadow crates (`orca-pty`, `orca-session`, `orca-git`, `orca-net`,
  `orca-relay`, `orca-text`, `orca-crypto`),
- the parity gate (TS↔Rust differential suite, currently green),
- the proof pipeline (ts2rust two-witness: `trustc` static proof + Node-TS
  differential),
- the client protocol (`orca-relay` terminal-stream framing, parity-proven), and
- a native app shell (`aterm-gui`) with more personality than the Electron one.

Each move below *reuses* that machinery rather than building new scaffolding.

---

## Move 1 — Extract `orca-daemon` as a pure Rust binary

**Start here. Weeks, not months.**

The daemon is already the heart of Orca: sessions, PTYs, headless engines, git
state. And the crates already exist:

| Concern | Crate | Status |
| --- | --- | --- |
| PTY spawn/IO | `orca-pty` | exists |
| Session model | `orca-session` | exists |
| Git state | `orca-git` | Trust-verified, in prod |
| Wire protocol | `orca-net` (NDJSON) | exists |
| Terminal-stream framing | `orca-relay` | parity-proven |

**Today** the daemon is TypeScript calling Rust through napi. **Inverted**, it is
a Rust process using `aterm-core` *as a crate* — **the napi boundary disappears
entirely from the hot path.**

Electron keeps talking to the daemon over the **existing socket protocol**, so
nothing user-visible changes. This is the lowest-risk, highest-leverage first
move: it removes a whole class of boundary cost and marshalling without touching
the UI.

This is also where the **autoformalization pipeline pays off**: the remaining TS
daemon logic is exactly the kind of bounded, testable code that ts2rust + the
parity gate migrate safely. The daemon is where "prove it, then port it" has the
best surface area.

**Deliverable to open the move:** a design doc + a daemon-extraction spike (a Rust
binary that owns one PTY + one `aterm-core` engine and answers the existing socket
protocol for a single session), run behind a flag against the real Electron
frontend.

---

## Move 2 — Two frontends, one daemon (the bridge)

Formalize the client protocol (`orca-relay` + the mobile/SSH work already pushed
this way), then **`aterm-gui` gains a "workspace mode"**: connect to
`orca-daemon`, render N sessions in native panes.

`aterm-gui` already has the panes-adjacent machinery to do this:

- settings v2,
- overlays and scenes,
- effects at full fidelity,
- fabric's "every session addressable from everywhere".

That ships a terminal-first **Orca Native** where the terminal experience is
**flawless** — zero canvas seam, effects at full fidelity, one coordinate system —
while **Electron Orca keeps the full IDE surface**.

Users choose per task. Both stay in sync because **state lives in the daemon**,
not in either frontend. This is the payoff of Move 1: once the daemon is the
single source of truth, a second frontend is additive, not a fork.

---

## Move 3 — Migrate IDE surfaces by value, not by ideology

Move IDE surfaces into the native shell **piece by piece, worst-seam-first**,
using the crates that are already proven:

- **Quick-open** → `orca-text` (ranking core already parity-proven)
- **Git / source-control panel** → `orca-git`
- **Agent orchestration views** → fabric (it is *made* for this)

### The honest hard parts — sequence these last

- **The editor.** Monaco is a decade of free web engineering. Two credible
  end-states: (a) embed a **webview island** for editor panes long-term, or (b)
  go **GPUI/zed-style** when you are ready to *own* text editing. Do not start
  here.
- **Accessibility.** **AccessKit** is the Rust answer. Budget real time — this is
  not a weekend.
- **i18n.** Same: real work, sequence it deliberately.

A **hybrid end-state** — native terminal/perf surfaces + web islands for
editor-heavy panels — is **not a compromise**. It is putting each toolchain where
it is unbeatable.

---

## What to explicitly NOT do

**Native child-window panes inside the Electron window** (NSView parenting /
shared-texture IOSurface tricks).

It is the worst of both worlds: you keep Electron *and* inherit focus / IME /
compositing edge cases. The wasm path stays the Electron terminal until **Move 2**
makes it moot — do not try to smuggle native surfaces into the Electron window in
the meantime.

---

## Sequencing summary

1. **Move 1 — `orca-daemon` as a pure Rust binary.** Removes the napi hot-path
   boundary. Nothing user-visible changes. Weeks, not months. **Start here.**
2. **Move 2 — Two frontends, one daemon.** `aterm-gui` workspace mode ships a
   flawless terminal-first Orca Native; Electron Orca keeps the full IDE. State
   lives in the daemon so both stay in sync.
3. **Move 3 — Migrate IDE surfaces by value.** Quick-open, git panel, orchestration
   views into the native shell using already-proven crates. Editor, a11y, i18n
   sequenced last; a native-terminal + web-editor hybrid is the honest, strong
   end-state.

**The through-line:** you already built the migration machinery — the shadow
crates, the parity gate, the proof pipeline, the protocol, and a native app shell
with more personality than the Electron one. This isn't a rewrite; it's a
reunification.
