---- MODULE PtyExitDelivery ----
(* PTY exit-delivery exactly-once protocol (per ptyId) — the core of orc's
   src/renderer/src/components/terminal-pane/pty-exit-delivery.ts +
   pty-pre-handler-buffer.ts + pty-dispatcher.ts, the site of the #7894 remount
   race and the exitedBeforeAttach class (v1.4.142 reconciliation).

   The hazard: a PTY's exit IPC and the primary handler's registration race, and
   a pane can REMOUNT (unregister + re-register) around an exit. The design must
   deliver every exit to the primary EXACTLY ONCE — never lost (a pre-handler
   buffer holds an early exit until a handler attaches), never doubled (a
   consumed-tombstone blocks a late buffer from re-delivering).

   Modeled abstractly for one ptyId: the PTY exits at most once; the primary
   handler registers/unregisters (remount) any number of times; ExitIPC fires
   once when the kernel reports exit. `delivered` counts primary invocations. *)
EXTENDS Naturals

VARIABLES
  ptyExited,     \* the kernel has reported the PTY exit (env fact)
  ipcFired,      \* the ExitIPC has been dispatched (consumes ptyExited once)
  primary,       \* a primary handler is currently registered
  buffered,      \* a pre-handler exit is buffered (early exit, no handler yet)
  recentExit,    \* recordRecentPtyExit fired: the exit is in the replay tracker
  tombstone,     \* this ptyId's exit was already consumed (blocks re-delivery)
  delivered      \* number of times the primary handler was invoked with the exit
vars == <<ptyExited, ipcFired, primary, buffered, recentExit, tombstone, delivered>>

TypeOK ==
  /\ ptyExited \in BOOLEAN
  /\ ipcFired \in BOOLEAN
  /\ primary \in BOOLEAN
  /\ buffered \in BOOLEAN
  /\ recentExit \in BOOLEAN
  /\ tombstone \in BOOLEAN
  /\ delivered \in 0..3

Init ==
  /\ ptyExited = FALSE
  /\ ipcFired = FALSE
  /\ primary = FALSE
  /\ buffered = FALSE
  /\ recentExit = FALSE
  /\ tombstone = FALSE
  /\ delivered = 0

(* Kernel reports the PTY exit (once). *)
Exit ==
  /\ ~ptyExited
  /\ ptyExited' = TRUE
  /\ UNCHANGED <<ipcFired, primary, buffered, recentExit, tombstone, delivered>>

(* Primary handler registers (mount / remount attach). On attach it drains a
   buffered early exit OR replays a recorded recent exit to the late handler —
   BUT only if not already tombstoned (the consumed-tombstone / hadPrimary guard).
   The buffered and recentExit paths are the two ways a late handler learns of an
   exit it missed live; the tombstone is what stops the SECOND path from firing
   after the first already delivered. *)
Register ==
  /\ ~primary
  /\ primary' = TRUE
  /\ IF (buffered \/ recentExit) /\ ~tombstone
       THEN delivered' = delivered + 1 /\ tombstone' = TRUE /\ buffered' = FALSE
       ELSE UNCHANGED <<delivered, tombstone, buffered>>
  /\ UNCHANGED <<ptyExited, ipcFired, recentExit>>

(* Pane remounts: the primary handler unregisters. The tombstone, buffer, and
   recent-exit record are per-ptyId module state that SURVIVES the unregister
   (losing this across remount was the #7894 bug class). *)
Unregister ==
  /\ primary
  /\ primary' = FALSE
  /\ UNCHANGED <<ptyExited, ipcFired, buffered, recentExit, tombstone, delivered>>

(* The exit IPC is dispatched (once, after the kernel exit). It deletes the
   handler from the map BEFORE invoking (one-shot), ALWAYS records the recent exit
   (recordRecentPtyExit fires regardless of a handler), then:
     primary present  -> deliver to primary, set tombstone (unless already);
     primary absent    -> buffer the exit (no-op if already tombstoned). *)
ExitIPC ==
  /\ ptyExited
  /\ ~ipcFired
  /\ ipcFired' = TRUE
  /\ recentExit' = TRUE
  /\ IF primary
       THEN /\ IF ~tombstone
                 THEN delivered' = delivered + 1 /\ tombstone' = TRUE
                 ELSE UNCHANGED <<delivered, tombstone>>
            /\ UNCHANGED buffered
       ELSE /\ IF ~tombstone THEN buffered' = TRUE ELSE UNCHANGED buffered
            /\ UNCHANGED <<delivered, tombstone>>
  /\ UNCHANGED <<ptyExited, primary>>

Next == Exit \/ Register \/ Unregister \/ ExitIPC

(* Fairness: once the exit fired the IPC eventually dispatches, and a handler
   eventually (re)attaches to drain a buffered exit. *)
Spec == Init /\ [][Next]_vars /\ WF_vars(ExitIPC) /\ WF_vars(Register)

(* SAFETY — exactly-once has two halves. NEVER doubled: *)
NeverDoubled == delivered <= 1

(* A buffered exit is always backed by a real exit + a set tombstone-or-pending;
   and a tombstone only exists once something was delivered or buffered. *)
TombstoneSound == tombstone => (delivered = 1 \/ buffered)

(* LIVENESS — never lost: once the PTY has exited, the primary is eventually
   invoked exactly once. (Requires the IPC to fire and a handler to attach.) *)
EventuallyDelivered == ptyExited ~> (delivered = 1)

(* The intended terminal state: exit delivered once, IPC fired, nothing buffered,
   and tombstoned (so any further replay/register is a no-op). primary may be
   TRUE or FALSE (the pane may have remounted away after delivery). *)
Done == ptyExited /\ ipcFired /\ delivered = 1 /\ ~buffered /\ tombstone
====
