---- MODULE PtyFlowControl ----
(* PTY producer flow-control pause/resume protocol — the hysteresis machine in
   orc src/main/ipc/pty-producer-flow-control.ts / rust orca-flow-control:
   main-process controller (pause > HIGH, resume < LOW, periodic re-assert) +
   daemon producer (pauses/resumes on delivered messages, FAILSAFE auto-resume
   if a pause is not re-asserted) over an in-order but LOSSY channel.

   Timer-abstracted v1: REASSERT/FAILSAFE expiries are nondeterministic actions,
   so the model covers every ordering of the two timers; WF encodes "a
   continuously-armed timer eventually fires". pending is region-abstracted.
   Controller state flips never block on channel room (sends are async in the
   real system; a full model channel only drops the message, mirroring loss).

   Model finding (kept as documentation): the unconditioned property
   (mainPaused /\ daemonPaused) ~> (~mainPaused /\ ~daemonPaused) is FALSE —
   ty exhibits the fair flood lasso FailsafeFire -> Drain -> DataArrives ->
   Reassert -> Deliver(pause): under sustained flooding the pipe stays
   throttled BY DESIGN. Resume liveness therefore conditions on quiescence. *)
EXTENDS Naturals, Sequences

CONSTANTS HIGH, LOW, MAXP, CHANCAP
ASSUME LOW < HIGH /\ HIGH < MAXP /\ LOW > 0 /\ CHANCAP > 0

VARIABLES pending,      \* undelivered chars, region-abstracted
          mainPaused,   \* controller's pause record for this pty
          daemonPaused, \* daemon Session.producerPaused
          failsafeArmed,\* daemon failsafe timer armed (re-armed per pause)
          chan,         \* in-order lossy control channel
          flooding      \* environment: is the producer still generating data?
vars == <<pending, mainPaused, daemonPaused, failsafeArmed, chan, flooding>>

TypeOK ==
  /\ pending \in 0..MAXP
  /\ mainPaused \in BOOLEAN
  /\ daemonPaused \in BOOLEAN
  /\ failsafeArmed \in BOOLEAN
  /\ chan \in Seq({"pause", "resume"})
  /\ Len(chan) <= CHANCAP
  /\ flooding \in BOOLEAN

(* Every pause delivery arms the failsafe and every daemon unpause disarms it,
   so a paused daemon always holds a live failsafe — the wedge-proof core. *)
PauseImpliesArmed == daemonPaused => failsafeArmed

Init ==
  /\ pending = 0
  /\ mainPaused = FALSE
  /\ daemonPaused = FALSE
  /\ failsafeArmed = FALSE
  /\ chan = <<>>
  /\ flooding = TRUE

(* Async send: the message rides if there is room, else it is dropped — the
   state flip NEVER blocks (matches the real controller; a full channel is just
   another loss mode of the best-effort transport). *)
Send(msg) == IF Len(chan) < CHANCAP THEN Append(chan, msg) ELSE chan

(* Data arrival (daemon reading) + the controller update fired on the change. *)
DataArrives ==
  /\ flooding
  /\ ~daemonPaused
  /\ pending < MAXP
  /\ pending' = pending + 1
  /\ IF ~mainPaused /\ pending + 1 > HIGH
       THEN mainPaused' = TRUE /\ chan' = Send("pause")
       ELSE UNCHANGED <<mainPaused, chan>>
  /\ UNCHANGED <<daemonPaused, failsafeArmed, flooding>>

(* Renderer flush/ack drains pending + the controller update on the change. *)
Drain ==
  /\ pending > 0
  /\ pending' = pending - 1
  /\ IF mainPaused /\ pending - 1 < LOW
       THEN mainPaused' = FALSE /\ chan' = Send("resume")
       ELSE UNCHANGED <<mainPaused, chan>>
  /\ UNCHANGED <<daemonPaused, failsafeArmed, flooding>>

(* Controller REASSERT timer: while paused and still above HIGH, re-send pause
   (defends the throttle against a lost pause / a failsafe-unpaused daemon). *)
Reassert ==
  /\ mainPaused
  /\ pending > HIGH
  /\ Len(chan) < CHANCAP
  /\ chan' = Append(chan, "pause")
  /\ UNCHANGED <<pending, mainPaused, daemonPaused, failsafeArmed, flooding>>

(* In-order delivery of the head control message. A pause (re-)arms the daemon
   failsafe; a resume disarms it. *)
Deliver ==
  /\ chan /= <<>>
  /\ chan' = Tail(chan)
  /\ IF Head(chan) = "pause"
       THEN daemonPaused' = TRUE /\ failsafeArmed' = TRUE
       ELSE daemonPaused' = FALSE /\ failsafeArmed' = FALSE
  /\ UNCHANGED <<pending, mainPaused, flooding>>

(* Best-effort transport: the head message is silently dropped. *)
Lose ==
  /\ chan /= <<>>
  /\ chan' = Tail(chan)
  /\ UNCHANGED <<pending, mainPaused, daemonPaused, failsafeArmed, flooding>>

(* Daemon FAILSAFE timer: a pause never re-asserted eventually self-resumes. *)
FailsafeFire ==
  /\ failsafeArmed
  /\ daemonPaused' = FALSE
  /\ failsafeArmed' = FALSE
  /\ UNCHANGED <<pending, mainPaused, chan, flooding>>

(* Session exit / releaseAll: the controller force-clears its pause. *)
Release ==
  /\ mainPaused
  /\ mainPaused' = FALSE
  /\ chan' = Send("resume")
  /\ UNCHANGED <<pending, daemonPaused, failsafeArmed, flooding>>

(* Environment quiesces: the producer stops generating (one-way). *)
EnvQuiesce ==
  /\ flooding
  /\ flooding' = FALSE
  /\ UNCHANGED <<pending, mainPaused, daemonPaused, failsafeArmed, chan>>

Next == DataArrives \/ Drain \/ Reassert \/ Deliver \/ Lose \/ FailsafeFire
          \/ Release \/ EnvQuiesce

(* Full-protocol fairness: delivery, draining, and the failsafe timer all make
   eventual progress. Reassert, Lose, EnvQuiesce stay UNFAIR (may never fire). *)
Spec == Init /\ [][Next]_vars
             /\ WF_vars(Deliver) /\ WF_vars(Drain) /\ WF_vars(FailsafeFire)

(* No-failsafe variant: the failsafe timer may never fire — NoWedge must FAIL
   here (a lost resume wedges the daemon forever). *)
SpecNoFailsafe == Init /\ [][Next]_vars /\ WF_vars(Deliver) /\ WF_vars(Drain)

(* LIVENESS: a paused daemon always eventually unpauses (wedge-freedom), even
   under sustained flooding and arbitrary message loss. *)
NoWedge == daemonPaused ~> ~daemonPaused

(* LIVENESS: once the producer quiesces, the pipe fully unthrottles. *)
QuiescentResume == (~flooding) ~> (~mainPaused /\ ~daemonPaused)

(* The one INTENDED final state: producer quiesced, pipe drained, all pause
   state clear, no messages in flight. Declared TERMINAL in the configs so any
   OTHER stuck state is still reported as a deadlock. *)
Quiesced == ~flooding /\ pending = 0 /\ chan = <<>>
              /\ ~mainPaused /\ ~daemonPaused /\ ~failsafeArmed
====
