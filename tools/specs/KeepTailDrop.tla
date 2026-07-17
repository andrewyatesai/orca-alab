---- MODULE KeepTailDrop ----
(* Background-session keep-tail DROP protocol — the drop-cap / keep-tail policy in
   orc src/main/daemon/daemon-stream-keep-tail-drop.ts + daemon-stream-data-batcher.ts,
   whose pure sizing core is rust orca-flow-control/src/keep_tail.rs.

   A backgrounded (hidden-pane) session's undelivered output is queued as a
   monitoring feed. Once its queued chars exceed the drop cap, the OLDEST bytes
   are dropped down to the keep-tail (a dataGap marker takes their place), so the
   feed stays tail-fresh and daemon memory stays bounded. The keep-tail SHRINKS
   as more droppable sessions compete for the shared global budget — bounding the
   AGGREGATE a reveal must drain — but never below the floor. Control entries are
   never dropped (region-abstracted away here: we track only the droppable data
   count q; salvage's <=4096-char re-add is likewise abstracted).

   Real sizing (keep_tail.rs):
     keepTail(n) = clamp(2_097_152 / max(1,n), [65_536, 524_288])   (2M / 64K / 512K)
     dropCap(n)  = 2 * keepTail(n)
   MODEL SCALING: BUDGET=8, FLOOR=2, CEIL=4 shrink 2M/64K/512K to keep the state
   space finite. This PRESERVES the clamp STRUCTURE (FLOOR <= CEIL <= BUDGET, an
   n-monotone shrink from CEIL down to the FLOOR) and integrality of keepTail/
   dropCap — it does NOT preserve the real ratios (real BUDGET/CEIL=4, CEIL/FLOOR=8;
   here 2 and 2). Only the qualitative bands drive the safety/liveness properties:
     KeepTail(1)=KeepTail(2)=CEIL=4, DropCap=8   (few sessions -> full tail)
     KeepTail(3)=KeepTail(4)=FLOOR=2, DropCap=4   (competition -> shrunk to floor)
   so the shrink bites crossing n=2 -> n=3, which is where the n-grow re-trim earns
   its keep. q is region-abstracted (a char count against the thresholds), n is the
   droppable-sessions-with-queued-data count (a small bounded env variable). *)
EXTENDS Naturals

CONSTANTS BUDGET, FLOOR, CEIL, MAXN, MAXQ
ASSUME FLOOR <= CEIL /\ CEIL <= BUDGET /\ FLOOR > 0 /\ MAXN >= 1 /\ MAXQ > 2 * CEIL

Min(a, b) == IF a < b THEN a ELSE b
Max(a, b) == IF a > b THEN a ELSE b

(* keep_tail.rs: clamp(BUDGET / max(1,n), [FLOOR, CEIL]). \div is floored, as the
   TS Math.floor / Rust u64 division are; max(1,n) guards n=0 exactly. *)
KeepTail(k) == Min(CEIL, Max(FLOOR, BUDGET \div Max(1, k)))
DropCap(k)  == 2 * KeepTail(k)

VARIABLES q,            \* the tracked session's queued droppable chars (region-abstracted)
          n,            \* droppable sessions WITH queued data — shrinks the shared budget
          flooding,     \* env: is the producer still generating output?
          justEnqueued  \* TRUE right after a drop-bearing step (Enqueue / n-grow re-trim)
vars == <<q, n, flooding, justEnqueued>>

TypeOK ==
  /\ q \in 0..MAXQ
  /\ n \in 1..MAXN
  /\ flooding \in BOOLEAN
  /\ justEnqueued \in BOOLEAN

Init ==
  /\ q = 0
  /\ n = 1
  /\ flooding = TRUE
  /\ justEnqueued = FALSE

(* Enqueue a data chunk of any size, then apply the drop rule in the SAME step —
   this mirrors the batcher, which checks `queued > dropCap` and calls
   dropOldestQueuedForSession synchronously inside the same enqueue call, so there
   is no observable pre-drop state. If the grown queue exceeds the cap it is
   thinned back down to the keep-tail. *)
Enqueue ==
  /\ flooding
  /\ q < MAXQ
  /\ \E grown \in (q + 1)..MAXQ:
       q' = (IF grown > DropCap(n) THEN KeepTail(n) ELSE grown)
  /\ justEnqueued' = TRUE
  /\ UNCHANGED <<n, flooding>>

(* Reveal / flush drains the feed to the client by any amount (down to empty). *)
Drain ==
  /\ q > 0
  /\ \E d \in 0..(q - 1): q' = d
  /\ justEnqueued' = FALSE
  /\ UNCHANGED <<n, flooding>>

(* Another droppable session starts queuing: the shared budget tightens, so the
   cap shrinks and sessions that already finished producing must be RE-TRIMMED —
   they never re-enter Enqueue on their own (batcher lastDroppableSessionCount
   branch). Modeled as the same synchronous grow-n-then-retrim of our session. *)
NGrow ==
  /\ flooding
  /\ n < MAXN
  /\ n' = n + 1
  /\ q' = (IF q > DropCap(n + 1) THEN KeepTail(n + 1) ELSE q)
  /\ justEnqueued' = TRUE
  /\ UNCHANGED flooding

(* A droppable session drains away: the budget loosens (cap grows), no re-trim
   needed. *)
NShrink ==
  /\ flooding
  /\ n > 1
  /\ n' = n - 1
  /\ UNCHANGED <<q, flooding, justEnqueued>>

(* Env quiesces: the producer stops generating (one-way). Also freezes the env
   session churn, so a fully drained idle system is a genuine terminal. *)
EnvStopFlooding ==
  /\ flooding
  /\ flooding' = FALSE
  /\ UNCHANGED <<q, n, justEnqueued>>

Next == Enqueue \/ Drain \/ NGrow \/ NShrink \/ EnvStopFlooding

(* Only Drain is fair: reveal/flush eventually makes progress. Enqueue, NGrow,
   NShrink, EnvStopFlooding stay UNFAIR (the producer/env may act arbitrarily). *)
Spec == Init /\ [][Next]_vars /\ WF_vars(Drain)

(* SAFETY — BoundedMemory: after a completed Enqueue+Drop (or n-grow re-trim), the
   queue is back within the drop cap. The drop rule is what makes this hold; drop
   the trim and the queue climbs past the cap unboundedly (see the Broken variant). *)
BoundedMemory == justEnqueued => q <= DropCap(n)

(* SAFETY — NeverBelowFloor: the keep-tail the drop trims a competing session down
   to is never below the floor, so heavy competition never starves a session below
   a full-screen tail. (q itself may sit below FLOOR when no drop has fired — the
   floor is on the DROP TARGET, not on every legitimate small queue.) *)
NeverBelowFloor == KeepTail(n) >= FLOOR

(* LIVENESS — EventualDrain: once the producer quiesces, a fair reveal/flush drains
   the whole feed to empty. *)
EventualDrain == (~flooding) ~> (q = 0)

(* The one intended terminal: producer quiesced and the feed fully drained. Declared
   TERMINAL so any OTHER stuck state is still reported as a deadlock. *)
Drained == ~flooding /\ q = 0
====
