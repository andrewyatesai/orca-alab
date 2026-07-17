---- MODULE PtyExitDelivery_nobar ----
EXTENDS Naturals
VARIABLES ptyExited, ipcFired, primary, buffered, recentExit, tombstone, delivered
vars == <<ptyExited, ipcFired, primary, buffered, recentExit, tombstone, delivered>>
TypeOK == ptyExited \in BOOLEAN /\ ipcFired \in BOOLEAN /\ primary \in BOOLEAN /\ buffered \in BOOLEAN /\ recentExit \in BOOLEAN /\ tombstone \in BOOLEAN /\ delivered \in 0..3
Init == ptyExited=FALSE /\ ipcFired=FALSE /\ primary=FALSE /\ buffered=FALSE /\ recentExit=FALSE /\ tombstone=FALSE /\ delivered=0
Exit == ~ptyExited /\ ptyExited'=TRUE /\ UNCHANGED <<ipcFired,primary,buffered,recentExit,tombstone,delivered>>
\* BUG: replay/drain on Register WITHOUT the ~tombstone guard -> re-delivers after a live delivery + remount
Register == ~primary /\ primary'=TRUE
  /\ IF (buffered \/ recentExit) THEN delivered'=delivered+1 /\ buffered'=FALSE ELSE UNCHANGED <<delivered,buffered>>
  /\ UNCHANGED <<ptyExited,ipcFired,recentExit,tombstone>>
Unregister == primary /\ primary'=FALSE /\ UNCHANGED <<ptyExited,ipcFired,buffered,recentExit,tombstone,delivered>>
ExitIPC == ptyExited /\ ~ipcFired /\ ipcFired'=TRUE /\ recentExit'=TRUE
  /\ IF primary THEN delivered'=delivered+1 /\ UNCHANGED buffered ELSE buffered'=TRUE /\ UNCHANGED delivered
  /\ UNCHANGED <<ptyExited,primary,tombstone>>
Next == Exit \/ Register \/ Unregister \/ ExitIPC
Spec == Init /\ [][Next]_vars /\ WF_vars(ExitIPC) /\ WF_vars(Register)
NeverDoubled == delivered <= 1
====
