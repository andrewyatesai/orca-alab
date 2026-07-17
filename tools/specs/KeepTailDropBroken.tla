---- MODULE KeepTailDropBroken ----
(* NEGATIVE CONTROL for KeepTailDrop. Identical to the real protocol EXCEPT the
   Enqueue step OMITS the drop: it grows the queue but never thins it down to the
   keep-tail. Everything else (the n-grow re-trim, drain, env) is unchanged, so the
   ONLY defect is the missing per-session drop.

   With the drop removed the queue climbs straight past the drop cap, so BoundedMemory
   MUST be violated by ty (at n=1: an Enqueue to grown=9..12 leaves q > DropCap(1)=8
   with justEnqueued=TRUE). If this run ever PASSES, the model has rotted and the
   spec's safety no longer depends on the drop rule — i.e. it would be worthless. *)
EXTENDS Naturals

CONSTANTS BUDGET, FLOOR, CEIL, MAXN, MAXQ
ASSUME FLOOR <= CEIL /\ CEIL <= BUDGET /\ FLOOR > 0 /\ MAXN >= 1 /\ MAXQ > 2 * CEIL

Min(a, b) == IF a < b THEN a ELSE b
Max(a, b) == IF a > b THEN a ELSE b
KeepTail(k) == Min(CEIL, Max(FLOOR, BUDGET \div Max(1, k)))
DropCap(k)  == 2 * KeepTail(k)

VARIABLES q, n, flooding, justEnqueued
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

(* BUG: grows the queue but NEVER trims it down to KeepTail(n) — the queue can
   exceed DropCap(n) without bound. *)
Enqueue ==
  /\ flooding
  /\ q < MAXQ
  /\ \E grown \in (q + 1)..MAXQ: q' = grown
  /\ justEnqueued' = TRUE
  /\ UNCHANGED <<n, flooding>>

Drain ==
  /\ q > 0
  /\ \E d \in 0..(q - 1): q' = d
  /\ justEnqueued' = FALSE
  /\ UNCHANGED <<n, flooding>>

NGrow ==
  /\ flooding
  /\ n < MAXN
  /\ n' = n + 1
  /\ q' = (IF q > DropCap(n + 1) THEN KeepTail(n + 1) ELSE q)
  /\ justEnqueued' = TRUE
  /\ UNCHANGED flooding

NShrink ==
  /\ flooding
  /\ n > 1
  /\ n' = n - 1
  /\ UNCHANGED <<q, flooding, justEnqueued>>

EnvStopFlooding ==
  /\ flooding
  /\ flooding' = FALSE
  /\ UNCHANGED <<q, n, justEnqueued>>

Next == Enqueue \/ Drain \/ NGrow \/ NShrink \/ EnvStopFlooding
Spec == Init /\ [][Next]_vars /\ WF_vars(Drain)

BoundedMemory == justEnqueued => q <= DropCap(n)
====
