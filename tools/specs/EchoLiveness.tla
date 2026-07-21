---- MODULE EchoLiveness ----
(* Keystroke -> visible-echo pipeline for orc's mosh-style predictive echo — the
   controller in src/renderer/src/lib/pane-manager/aterm/aterm-prediction-echo.ts
   driving the engine predictor over the DEFAULT off-main worker facade
   (aterm-worker-predict-facade.ts). The property this defends is FELT LATENCY:
   the delay between typing a key and SEEING it on screen.

   A typed char has two visibility paths:
     - the local predicted ghost (armed predictor) — painted within GHOST_MAX,
       INDEPENDENT of the network, because it never leaves the renderer; and
     - the real PTY/SSH echo — painted after the full round-trip, 1..RTT_MAX.
   With RTT_MAX >> GHOST_MAX (a laggy SSH shell), only the ghost keeps felt
   latency low. The ghost deadline is enforced: time cannot advance past
   GHOST_MAX while an armed ghost is still pending (mirrors the controller's
   glitch-expiry timer that guarantees the ghost is painted, then self-heals).

   Model finding (kept as documentation): BoundedFeltLatency —
   [](typed /\ clock >= GHOST_MAX => Visible) — HOLDS iff ARMED. With ARMED=FALSE
   (the EXACT inert-worker-facade regression: the controller's capability probe
   saw engine=null when predict_* went missing, so ShowGhost never fires) the
   ONLY visibility path is the real echo, and felt latency degrades to the full
   round-trip (up to RTT_MAX). BoundedFeltLatency then FAILS while Liveness
   (typed ~> Visible) STILL HOLDS — the char DOES eventually appear, just late.
   That gap IS the silent typing-lag bug: no crash, output still correct, but the
   latency mask is gone. The negative-control cfg (ARMED=FALSE) exists so the
   model keeps PROVING it can still catch that regression. *)
EXTENDS Naturals

CONSTANTS GHOST_MAX,  \* max ticks from Type to the local ghost being painted
          RTT_MAX,    \* max ticks from Type to the real PTY/SSH echo arriving
          ARMED       \* is the predictor armed? (FALSE = the inert-facade regression)
ASSUME GHOST_MAX \in Nat /\ RTT_MAX \in Nat
ASSUME GHOST_MAX >= 1 /\ GHOST_MAX < RTT_MAX  \* the ghost must beat the round-trip
ASSUME ARMED \in BOOLEAN

VARIABLES typed,      \* a key was typed and its lifecycle is not yet reset
          ghostShown, \* the local predicted ghost is painted (armed path only)
          echoShown,  \* the real PTY/SSH echo has arrived and is painted
          clock       \* ticks elapsed since the key was typed (0 at Type)
vars == <<typed, ghostShown, echoShown, clock>>

(* The char is on screen once EITHER path has painted it. *)
Visible == ghostShown \/ echoShown

TypeOK ==
  /\ typed \in BOOLEAN
  /\ ghostShown \in BOOLEAN
  /\ echoShown \in BOOLEAN
  /\ clock \in 0..RTT_MAX

Init ==
  /\ typed = FALSE
  /\ ghostShown = FALSE
  /\ echoShown = FALSE
  /\ clock = 0

(* A key is typed: start its visibility clock. One in-flight keystroke at a time
   (typed is a flag, not a count) — enough to prove the felt-latency bound and
   keep the state space tiny. *)
Type ==
  /\ ~typed
  /\ typed' = TRUE
  /\ clock' = 0
  /\ ghostShown' = FALSE
  /\ echoShown' = FALSE

(* Local predicted echo: the renderer paints the ghost. Only when ARMED — the
   inert facade never reaches this action, which is the whole regression. *)
ShowGhost ==
  /\ ARMED
  /\ typed
  /\ ~ghostShown
  /\ ghostShown' = TRUE
  /\ UNCHANGED <<typed, echoShown, clock>>

(* Real PTY/SSH echo: arrives after at least one round-trip tick (>= 1) and is
   forced to land by RTT_MAX (the clock cannot advance past RTT_MAX — see Tick). *)
ShowEcho ==
  /\ typed
  /\ ~echoShown
  /\ clock >= 1
  /\ echoShown' = TRUE
  /\ UNCHANGED <<typed, ghostShown, clock>>

(* Time passes. The clock is bounded by RTT_MAX (the echo is guaranteed by then).
   DEADLINE ENFORCEMENT: while an ARMED ghost is still pending, a tick that would
   bring the clock to/through GHOST_MAX is disabled — the ghost must paint first.
   So a state with (ARMED /\ typed /\ ~ghostShown /\ clock >= GHOST_MAX) is
   unreachable, which is exactly what BoundedFeltLatency asserts. *)
Tick ==
  /\ clock < RTT_MAX
  /\ ~(ARMED /\ typed /\ ~ghostShown /\ clock + 1 >= GHOST_MAX)
  /\ clock' = clock + 1
  /\ UNCHANGED <<typed, ghostShown, echoShown>>

(* The keystroke lifecycle completes once the REAL char is confirmed on screen;
   the pane is ready for the next key. (Confirmation gates on the echo, never on
   our own guess — the password-prompt / no-echo safety.) *)
Reset ==
  /\ typed
  /\ echoShown
  /\ typed' = FALSE
  /\ ghostShown' = FALSE
  /\ echoShown' = FALSE
  /\ clock' = 0

Next == Type \/ ShowGhost \/ ShowEcho \/ Tick \/ Reset

(* Fairness: the ghost paints, the echo arrives, and time advances. Under these,
   every typed key eventually becomes Visible on BOTH configs (armed via the
   ghost, disarmed via the slow echo) — Liveness holds either way; only
   BoundedFeltLatency separates them. *)
Spec == Init /\ [][Next]_vars
             /\ WF_vars(ShowGhost) /\ WF_vars(ShowEcho) /\ WF_vars(Tick)

(* SAFETY — bounded felt latency: once GHOST_MAX ticks have elapsed since a key
   was typed, it is already Visible. Independent of RTT_MAX. HOLDS iff ARMED. *)
FeltLatencyBound == (typed /\ clock >= GHOST_MAX) => Visible
BoundedFeltLatency == [](FeltLatencyBound)

(* LIVENESS — a typed key always eventually becomes visible. HOLDS on BOTH
   configs (armed: the ghost; disarmed: the slow real echo). Present so the
   negative control proves it is SPECIFICALLY the felt-latency bound that the
   inert predictor loses — not visibility altogether. *)
Liveness == typed ~> Visible
====
