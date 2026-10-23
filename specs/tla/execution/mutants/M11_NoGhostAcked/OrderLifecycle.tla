----------------------------- MODULE OrderLifecycle -----------------------------
\* Execution v0 order lifecycle, v3.1. Base: reports/execution-v0-spec.md v3
\* (sha256 406177757097b590a8607cd9a71d0ccd2dcef06fdf2c444cc17c1bca4b549dd7) plus the
\* EL-approved v3.1 fixes F1, F2/F3, F5, F7, F8, F9 (see README "v3.1").
\*
\* The local machine has the 9 v3 states and 13 events (v3's 12 + QueryExpired).
\* Allowed(s, e) is the 117-cell matrix: a non-empty set is the legal result set,
\* {} is the typed IllegalTransition. Every lifecycle step goes through Allowed and
\* records the cell it took in `last`.
\*
\* Not modeled: prices, filters, rate-limit backoff, risk event replay seq numbers,
\* payload bytes, sim/paper. Quantities are abstract units 0..MaxQty.

EXTENDS Integers, FiniteSets, TLC

CONSTANTS Intents,      \* intent ids; one slot each
          MaxQty,       \* max |position| and max order qty (abstract units)
          MaxAge,       \* Limits::max_approval_age in abstract ticks
          MaxEpoch,     \* bound on the number of kills
          InitPos,      \* possible restored settled positions
          InitForeign,  \* possible "venue has an order the book does not know"
          MaxAttempts,  \* F7: attempts per intent; ClientOrderId = <<intent, attempt>>
          RecvWindow,   \* F3: recvWindow in abstract clock ticks
          Margin        \* F3: safety margin (clock skew bound) in ticks

ASSUME MaxQty \in Nat /\ MaxQty >= 1 /\ MaxAge \in Nat /\ MaxEpoch \in Nat
ASSUME InitPos \subseteq -MaxQty..MaxQty /\ InitForeign \subseteq BOOLEAN
ASSUME MaxAttempts \in Nat /\ MaxAttempts >= 1 /\ RecvWindow \in Nat /\ Margin \in Nat

States == {"PendingNew", "Acked", "PartiallyFilled", "Filled", "PendingCancel",
           "Canceled", "Rejected", "Expired", "Unknown"}
Events == {"Ack", "Fill", "CancelRequested", "CancelDenied", "CancelConfirmed",
           "Reject", "Expire", "Timeout", "QueryFilled", "QueryCanceled",
           "QueryExpired", "QueryNotFound", "RaceFill"}
Working  == {"PendingNew", "Acked", "PartiallyFilled", "PendingCancel"}
Terminal == {"Filled", "Canceled", "Rejected", "Expired"}

\* v3.1 matrix. {} = IllegalTransition. Cells changed from v3 are tagged.
Allowed(s, e) ==
  CASE e = "Ack" ->
         IF s = "PendingNew" THEN {"Acked"}
         ELSE IF s \in {"Acked", "PartiallyFilled", "PendingCancel"} THEN {s} ELSE {}
    [] e = "Fill" ->
         IF s \in {"PendingNew", "Acked", "PartiallyFilled"} THEN {"PartiallyFilled", "Filled"}
         ELSE IF s = "PendingCancel" THEN {"PendingCancel", "Filled"} ELSE {}
    [] e = "CancelRequested" ->
         IF s = "PendingNew" THEN {"PendingCancel", "Canceled"}          \* F9
         ELSE IF s \in Working THEN {"PendingCancel"}
         ELSE IF s = "Unknown" THEN {"Unknown"} ELSE {}
    [] e = "CancelDenied"    ->
         IF s = "PendingCancel" THEN {"Acked", "PartiallyFilled", "Unknown"} ELSE {}  \* F5
    [] e = "CancelConfirmed" -> IF s = "PendingCancel" THEN {"Canceled"} ELSE {}
    [] e = "Reject"          -> IF s = "PendingNew" THEN {"Rejected"} ELSE {}
    [] e = "Expire"          -> IF s \in Working THEN {"Expired"} ELSE {}
    [] e = "Timeout"         -> IF s \in Working \cup {"Unknown"} THEN {"Unknown"} ELSE {}
    [] e = "QueryFilled"     -> IF s = "Unknown" THEN {"Filled"} ELSE {}
    [] e = "QueryCanceled"   -> IF s = "Unknown" THEN {"Canceled"} ELSE {}
    [] e = "QueryExpired"    -> IF s = "Unknown" THEN {"Expired"} ELSE {}        \* F1 (new)
    [] e = "QueryNotFound"   ->                                                 \* F2/F3/F7
         IF s = "Unknown" THEN {"PendingNew", "Rejected", "Unknown"} ELSE {}
    [] e = "RaceFill"        -> IF s = "Canceled" THEN {"Canceled"} ELSE {}

Legal(s, e) == Allowed(s, e) # {}

\* TLC prints the whole matrix once at startup; gen_transitions.py parses it.
PrintMatrix ==
  \A s \in States : \A e \in Events : PrintT(<<"CELL", s, e, Allowed(s, e)>>)
ASSUME PrintMatrix

Att   == 0..(MaxAttempts - 1)
Bound == RecvWindow + Margin                 \* F3: NotFound authority bound
CID(i, a) == <<i, a>>                        \* F7: fresh id per attempt

Sign(x) == IF x > 0 THEN 1 ELSE IF x < 0 THEN -1 ELSE 0
Abs(x)  == IF x < 0 THEN -x ELSE x

RECURSIVE SumOver(_, _)
SumOver(f, S) == IF S = {} THEN 0
                 ELSE LET x == CHOOSE y \in S : TRUE IN f[x] + SumOver(f, S \ {x})

VARIABLES
  o,           \* [Intents -> local order record] (current attempt)
  v,           \* [Intents -> [Att -> venue record]]: venue truth per ClientOrderId
  ap,          \* [Intents -> BOOLEAN]: held approval's halt_epoch = current halt_epoch
  gh,          \* [Intents -> ghost record] (checks only; never read by actions' guards)
  halted,      \* execution halt latch
  flag,        \* operator alert raised (typed Alert)
  riskHalted,  \* risk-side halt (drawdown/kill in PortfolioSnapshot)
  kills,       \* kills so far (bounded by MaxEpoch)
  killed,      \* kill sequence in progress
  lpos,        \* risk's authoritative settled position (from processed receipts)
  pos,         \* venue settled position
  foreign,     \* venue has an open order unknown to the book (never adopted)
  fresh,       \* last reconciliation matched and nothing changed since
  last         \* last lifecycle cell taken: <<from, event, result>>

vars == <<o, v, ap, gh, halted, flag, riskHalted, kills, killed, lpos, pos, foreign,
          fresh, last>>

\* halt_epoch abstraction (F8): epochs only increase and approvals are compared to the
\* current epoch only for equality, so the observable part of halt_epoch is, per held
\* approval, "aEp = halt_epoch" (ap[i]). A bump sets every ap[i] to FALSE; an approval
\* issued now sets ap[i] to TRUE. This is exact, and keeps the state space finite.

EmptyO == [st |-> "None", qty |-> 0, side |-> 0, ro |-> FALSE, leg |-> "none",
           cum |-> 0, sent |-> FALSE, aAge |-> 0, sh |-> FALSE,
           resp |-> "none", cxl |-> FALSE, cresp |-> "none", cxlReq |-> FALSE,
           rem |-> -1, att |-> 0, el |-> 0, acked |-> FALSE]
\* st    local state ("None" = slot unused; not part of the 9-state machine)
\* sent  current attempt dispatched;  sh  halted when last dispatched;  aAge approval age
\* resp  submit response in flight ("ack"/"rej");  cxl  cancel request in flight
\* cresp cancel response in flight ("conf"/"deny");  rem  emitted Cancelled.remaining
\* att   attempt number (F7);  el  clock ticks since the last send of this CID (F3)
\* acked an Ack for this CID was processed (F5)
EmptyV == [vst |-> "none", vcum |-> 0, vsubs |-> 0, infl |-> FALSE]
\* vst   venue status ("none","open","filled","canceled","expired"); vcum venue fills
\* vsubs NEWs the venue received for this CID;  infl  a NEW for this CID still in flight
EmptyG == [ph |-> FALSE, pk |-> FALSE, ks |-> FALSE, xs |-> FALSE]
\* ph  a halt happened since the held approval was issued (F8 ghost)
\* xs  a submit used an approval issued before a halt (F8 violation witness)
\* pk  the order existed when kill started (F9 ghost)
\* ks  a submit happened for an order created before a kill (F9 violation witness)

V(i) == v[i][o[i].att]

Init ==
  /\ o = [i \in Intents |-> EmptyO]
  /\ v = [i \in Intents |-> [a \in Att |-> EmptyV]]
  /\ ap = [i \in Intents |-> FALSE]
  /\ gh = [i \in Intents |-> EmptyG]
  /\ halted = TRUE              \* fresh/restored books start halted
  /\ flag = FALSE
  /\ riskHalted = FALSE
  /\ kills = 0
  /\ killed = FALSE
  /\ pos \in InitPos
  /\ lpos = pos
  /\ foreign \in InitForeign
  /\ fresh = FALSE
  /\ last = <<"-", "-", "-">>

-------------------------------------------------------------------------------
\* Halt, halt_epoch and ghosts

Bump == ap' = [j \in Intents |-> FALSE]                 \* F8: every halt bumps the epoch
MarkHalt(g) == [j \in Intents |-> [g[j] EXCEPT !.ph = TRUE]]
GhSubmit(g, i) == [g EXCEPT ![i].ks = @ \/ g[i].pk, ![i].xs = @ \/ g[i].ph, ![i].ph = FALSE]

Halt  == halted' = TRUE /\ flag' = TRUE /\ Bump
HaltG == Halt /\ gh' = MarkHalt(gh)

-------------------------------------------------------------------------------
\* Exposure and gate

OpenLocal(i) == o[i].st \in {"Acked", "PartiallyFilled", "PendingCancel"}
                \/ (o[i].st = "PendingNew" /\ o[i].sent)
PendingOf(i) == IF OpenLocal(i) \/ o[i].st = "Unknown"
                THEN o[i].side * (o[i].qty - o[i].cum) ELSE 0
PendingExcept(i) == SumOver([j \in Intents |-> IF j = i THEN 0 ELSE PendingOf(j)], Intents)
ExposureKnown == ~foreign /\ \A j \in Intents : o[j].st # "Unknown"

\* strictly reduces |position + pending| and cannot cross zero
StrictReduce(i) ==
  LET x == lpos + PendingExcept(i)
      y == x + o[i].side * (o[i].qty - o[i].cum)
  IN  Abs(y) < Abs(x) /\ Sign(y) * Sign(x) >= 0

ApprovalFresh(i) == ap[i] /\ o[i].aAge <= MaxAge

GateOK(i) ==
  /\ ApprovalFresh(i)
  /\ \/ (~halted /\ ~riskHalted)
     \/ (o[i].ro /\ StrictReduce(i) /\ ExposureKnown)

\* abstract risk::check (EL decision: a strictly reducing reduce-only order is always
\* approvable, even while halted)
RiskApproves(ro, side, qty, x) ==
  IF ro /\ Abs(x + side * qty) < Abs(x) /\ Sign(x + side * qty) * Sign(x) >= 0
  THEN TRUE
  ELSE ~halted /\ ~riskHalted

Quiet(j) == /\ o[j].resp = "none" /\ ~o[j].cxl /\ o[j].cresp = "none"
            /\ \A a \in Att : ~v[j][a].infl
            /\ (o[j].st # "Unknown" =>
                  /\ o[j].cum = V(j).vcum
                  /\ (V(j).vst = "expired" => o[j].st = "Expired"))
AllQuiet == \A j \in Intents :
              /\ o[j].st \in {"None"} \cup Terminal
              /\ Quiet(j)
              /\ (o[j].st \in {"Canceled", "Expired"} => o[j].rem # -1)
FreeIntent == {i \in Intents : o[i].st = "None"}

NoOpenOrders == /\ ~foreign
                /\ \A j \in Intents : /\ \A a \in Att : v[j][a].vst # "open" /\ ~v[j][a].infl
                                      /\ o[j].st \notin Working \cup {"Unknown"}
KillDone == killed /\ NoOpenOrders /\ pos = 0

-------------------------------------------------------------------------------
\* Planning (normal mode) and the zero-crossing split

Plan(i, t) ==
  /\ ~halted /\ ~riskHalted /\ ~killed /\ fresh
  /\ o[i].st = "None" /\ AllQuiet
  /\ t # lpos
  /\ LET cross == lpos # 0 /\ t # 0 /\ Sign(t) # Sign(lpos)
         qty   == IF cross THEN Abs(lpos) ELSE Abs(t - lpos)
         side  == IF cross THEN -Sign(lpos) ELSE Sign(t - lpos)
         ro    == cross \/ (Abs(t) < Abs(lpos) /\ Sign(t) * Sign(lpos) >= 0)
     IN  /\ RiskApproves(ro, side, qty, lpos)
         /\ o' = [o EXCEPT ![i] = [EmptyO EXCEPT !.st = "PendingNew", !.qty = qty,
                     !.side = side, !.ro = ro,
                     !.leg = IF cross THEN "close" ELSE IF ro THEN "reduce" ELSE "open"]]
  /\ ap' = [ap EXCEPT ![i] = TRUE]
  /\ gh' = [gh EXCEPT ![i] = EmptyG]
  /\ UNCHANGED <<v, halted, flag, riskHalted, kills, killed, lpos, pos, foreign, fresh, last>>
\* Crossing zero only creates the reduce-only close leg. The open leg is a separate
\* intent created by a later Plan once the close is terminal, drained and reconciled.

-------------------------------------------------------------------------------
\* Dispatch (submit) through the RiskGate

DispatchPre(i) == o[i].st = "PendingNew" /\ ~o[i].sent /\ GateOK(i)
\* An approval is single-use: dispatch consumes it (a retry needs a new approval).
Consume(i) == ap' = [ap EXCEPT ![i] = FALSE]
SentRec(i) == [o[i] EXCEPT !.sent = TRUE, !.sh = halted \/ riskHalted, !.el = 0]

DispatchAck(i) ==
  /\ DispatchPre(i)
  /\ o' = [o EXCEPT ![i] = [SentRec(i) EXCEPT !.resp = "ack"]]
  /\ v' = [v EXCEPT ![i][o[i].att] = [@ EXCEPT !.vst = "open", !.vsubs = @ + 1]]
  /\ Consume(i)
  /\ gh' = GhSubmit(gh, i)
  /\ fresh' = FALSE
  /\ UNCHANGED <<halted, flag, riskHalted, kills, killed, lpos, pos, foreign, last>>

DispatchRej(i) ==   \* definitive venue rejection (e.g. -2022 reduce-only rejected)
  /\ DispatchPre(i)
  /\ o' = [o EXCEPT ![i] = [SentRec(i) EXCEPT !.resp = "rej"]]
  /\ Consume(i)
  /\ gh' = GhSubmit(gh, i)
  /\ fresh' = FALSE
  /\ UNCHANGED <<v, halted, flag, riskHalted, kills, killed, lpos, pos, foreign, last>>

\* Timeout (-1007 "send status unknown"): the NEW may have landed, may still be in
\* flight, or may be dropped. Cell (PendingNew, Timeout).
DispatchTimeout(i, how) ==
  /\ DispatchPre(i)
  /\ how \in {"landed", "inflight", "dropped"}
  /\ o' = [o EXCEPT ![i] = [SentRec(i) EXCEPT !.st = "Unknown"]]
  /\ v' = [v EXCEPT ![i][o[i].att] = [@ EXCEPT
              !.vst   = IF how = "landed" THEN "open" ELSE @,
              !.vsubs = IF how = "landed" THEN @ + 1 ELSE @,
              !.infl  = (how = "inflight")]]
  /\ Halt
  /\ gh' = MarkHalt(GhSubmit(gh, i))
  /\ last' = <<"PendingNew", "Timeout", "Unknown">>
  /\ fresh' = FALSE
  /\ UNCHANGED <<riskHalted, kills, killed, lpos, pos, foreign>>

\* Definite refusal of an unsent order whose approval is stale or from an old epoch.
\* Cell (PendingNew, Reject).
DispatchRefuse(i) ==
  /\ o[i].st = "PendingNew" /\ ~o[i].sent /\ ~ApprovalFresh(i)
  /\ o' = [o EXCEPT ![i].st = "Rejected"]
  /\ last' = <<"PendingNew", "Reject", "Rejected">>
  /\ fresh' = FALSE
  /\ UNCHANGED <<v, ap, gh, halted, flag, riskHalted, kills, killed, lpos, pos, foreign>>

\* F8: an unsent order whose approval went stale (age or epoch) may be re-approved at
\* the current epoch by risk::check. Not during a kill (except the kill's flatten).
Reapprove(i) ==
  /\ o[i].st = "PendingNew" /\ ~o[i].sent /\ ~ApprovalFresh(i)
  /\ ~killed \/ o[i].leg = "flatten"
  /\ RiskApproves(o[i].ro, o[i].side, o[i].qty - o[i].cum, lpos + PendingExcept(i))
  /\ ap' = [ap EXCEPT ![i] = TRUE]
  /\ o' = [o EXCEPT ![i].aAge = 0]
  /\ gh' = [gh EXCEPT ![i].ph = FALSE]
  /\ UNCHANGED <<v, halted, flag, riskHalted, kills, killed, lpos, pos, foreign, fresh, last>>

Tick(i) ==   \* injected time ages an unsent approval
  /\ o[i].st = "PendingNew" /\ ~o[i].sent /\ o[i].aAge <= MaxAge
  /\ o' = [o EXCEPT ![i].aAge = @ + 1]
  /\ UNCHANGED <<v, ap, gh, halted, flag, riskHalted, kills, killed, lpos, pos, foreign,
                 fresh, last>>

\* F3: abstract clock for an Unknown order: ticks since the last send of its CID,
\* saturating at Bound + 1. Physical assumption (recvWindow + skew <= Margin): a NEW
\* still in flight is accepted or discarded by the venue before el exceeds Bound.
TickClock(i) ==
  /\ o[i].st = "Unknown" /\ o[i].el <= Bound
  /\ o[i].el = Bound => ~V(i).infl
  /\ o' = [o EXCEPT ![i].el = @ + 1]
  /\ UNCHANGED <<v, ap, gh, halted, flag, riskHalted, kills, killed, lpos, pos, foreign,
                 fresh, last>>

-------------------------------------------------------------------------------
\* Venue, per ClientOrderId <<i, a>>

VLand(i, a) ==   \* an in-flight NEW reaches the matching engine (within recvWindow)
  /\ v[i][a].infl
  /\ a = o[i].att => o[i].el <= Bound
  /\ v' = [v EXCEPT ![i][a] = [@ EXCEPT !.infl = FALSE, !.vsubs = @ + 1,
                                        !.vst = IF @ = "none" THEN "open" ELSE @]]
  /\ fresh' = FALSE
  /\ UNCHANGED <<o, ap, gh, halted, flag, riskHalted, kills, killed, lpos, pos, foreign, last>>

VDrop(i, a) ==   \* the in-flight NEW is discarded (outside recvWindow, -1021)
  /\ v[i][a].infl
  /\ v' = [v EXCEPT ![i][a].infl = FALSE]
  /\ fresh' = FALSE
  /\ UNCHANGED <<o, ap, gh, halted, flag, riskHalted, kills, killed, lpos, pos, foreign, last>>

VFill(i, a) ==
  /\ v[i][a].vst = "open" /\ v[i][a].vcum < o[i].qty
  /\ v' = [v EXCEPT ![i][a] = [@ EXCEPT !.vcum = @ + 1,
                    !.vst = IF v[i][a].vcum + 1 = o[i].qty THEN "filled" ELSE "open"]]
  /\ pos' = pos + o[i].side
  /\ fresh' = FALSE
  /\ UNCHANGED <<o, ap, gh, halted, flag, riskHalted, kills, killed, lpos, foreign, last>>

VExpire(i, a) ==   \* IOC/GTX/market expiry, EXPIRED or EXPIRED_IN_MATCH
  /\ v[i][a].vst = "open"
  /\ v' = [v EXCEPT ![i][a].vst = "expired"]
  /\ fresh' = FALSE
  /\ UNCHANGED <<o, ap, gh, halted, flag, riskHalted, kills, killed, lpos, pos, foreign, last>>

VCancel(i) ==   \* the cancel request for the current CID reaches the venue
  /\ o[i].cxl
  /\ o' = [o EXCEPT ![i].cxl = FALSE,
                    ![i].cresp = IF V(i).vst = "open" THEN "conf" ELSE "deny"]
  /\ v' = [v EXCEPT ![i][o[i].att].vst = IF @ = "open" THEN "canceled" ELSE @]
  /\ fresh' = FALSE
  /\ UNCHANGED <<ap, gh, halted, flag, riskHalted, kills, killed, lpos, pos, foreign, last>>

\* F2: Query Order stops finding CANCELED/EXPIRED orders with no fills ~3 days after
\* creation (-2013). "Much later" is el > Bound in the abstract clock.
VPurge(i) ==
  /\ o[i].st = "Unknown" /\ o[i].el > Bound
  /\ V(i).vst \in {"canceled", "expired"} /\ V(i).vcum = 0
  /\ v' = [v EXCEPT ![i][o[i].att].vst = "none"]
  /\ fresh' = FALSE
  /\ UNCHANGED <<o, ap, gh, halted, flag, riskHalted, kills, killed, lpos, pos, foreign, last>>

-------------------------------------------------------------------------------
\* Venue-consistent reports into the lifecycle (current CID). A report whose cell is
\* illegal is a typed IllegalTransition: order unchanged, halt + alert, recorded as
\* result "IllegalV".

IllegalV(i, e, rec) ==
  /\ o' = [o EXCEPT ![i] = rec]
  /\ HaltG
  /\ last' = <<o[i].st, e, "IllegalV">>
  /\ UNCHANGED <<v, riskHalted, kills, killed, lpos, pos, foreign>>

DAck(i) ==
  /\ o[i].resp = "ack"
  /\ fresh' = FALSE
  /\ IF Legal(o[i].st, "Ack")
     THEN LET to == IF o[i].st = "PendingNew" THEN "Acked" ELSE o[i].st IN
          /\ o' = [o EXCEPT ![i].resp = "none", ![i].st = to, ![i].acked = TRUE]
          /\ last' = <<o[i].st, "Ack", to>>
          /\ UNCHANGED <<v, ap, gh, halted, flag, riskHalted, kills, killed, lpos, pos, foreign>>
     ELSE IllegalV(i, "Ack", [o[i] EXCEPT !.resp = "none"])

DRej(i) ==
  /\ o[i].resp = "rej"
  /\ fresh' = FALSE
  /\ IF Legal(o[i].st, "Reject") /\ o[i].cum = 0
     THEN /\ o' = [o EXCEPT ![i].resp = "none", ![i].st = "Rejected", ![i].sent = FALSE]
          /\ last' = <<o[i].st, "Reject", "Rejected">>
          /\ UNCHANGED <<v, ap, gh, halted, flag, riskHalted, kills, killed, lpos, pos, foreign>>
     ELSE IllegalV(i, "Reject", [o[i] EXCEPT !.resp = "none"])

\* One evidenced individual fill. On Canceled the same report is RaceFill.
\* Overfill validation: cum + 1 <= qty.
DFill(i) ==
  /\ V(i).vcum > o[i].cum
  /\ o[i].st \in States
  /\ fresh' = FALSE
  /\ LET e == IF o[i].st = "Canceled" THEN "RaceFill" ELSE "Fill" IN
     IF Legal(o[i].st, e) /\ o[i].cum + 1 <= o[i].qty
     THEN LET to == IF e = "RaceFill" THEN "Canceled"
                    ELSE IF o[i].cum + 1 = o[i].qty THEN "Filled"
                    ELSE IF o[i].st = "PendingCancel" THEN "PendingCancel"
                    ELSE "PartiallyFilled" IN
          /\ o' = [o EXCEPT ![i].cum = @ + 1, ![i].st = to]
          /\ lpos' = lpos + o[i].side
          /\ last' = <<o[i].st, e, to>>
          /\ UNCHANGED <<v, ap, gh, halted, flag, riskHalted, kills, killed, pos, foreign>>
     ELSE IllegalV(i, e, o[i])

\* F5: a CancelDenied for an order with no processed Ack and no fill does not prove
\* the order exists at the venue: it goes to Unknown (halt, query), not to Acked.
DCresp(i) ==
  /\ o[i].cresp # "none"
  /\ fresh' = FALSE
  /\ LET e == IF o[i].cresp = "conf" THEN "CancelConfirmed" ELSE "CancelDenied" IN
     IF Legal(o[i].st, e)
     THEN LET to == IF e = "CancelConfirmed" THEN "Canceled"
                    ELSE IF o[i].cum > 0 THEN "PartiallyFilled"
                    ELSE "Acked" IN
          /\ o' = [o EXCEPT ![i].cresp = "none", ![i].st = to]
          /\ last' = <<o[i].st, e, to>>
          /\ IF to = "Unknown" THEN HaltG ELSE UNCHANGED <<ap, gh, halted, flag>>
          /\ UNCHANGED <<v, riskHalted, kills, killed, lpos, pos, foreign>>
     ELSE IllegalV(i, e, [o[i] EXCEPT !.cresp = "none"])

\* Expire only after outstanding committed fills drained (cum = vcum).
DExpire(i) ==
  /\ V(i).vst = "expired" /\ o[i].cum = V(i).vcum
  /\ o[i].st \in States \ Terminal
  /\ fresh' = FALSE
  /\ IF Legal(o[i].st, "Expire")
     THEN /\ o' = [o EXCEPT ![i].st = "Expired"]
          /\ last' = <<o[i].st, "Expire", "Expired">>
          /\ UNCHANGED <<v, ap, gh, halted, flag, riskHalted, kills, killed, lpos, pos, foreign>>
     ELSE IllegalV(i, "Expire", o[i])

\* By-ID query (origClientOrderId = current CID) of an Unknown order. Complete
\* receipts are processed once (cum := vcum). None of these clears the latch.
QFilled(i) ==
  /\ o[i].st = "Unknown" /\ V(i).vst = "filled"
  /\ o' = [o EXCEPT ![i].st = "Filled", ![i].cum = V(i).vcum, ![i].el = 0]
  /\ lpos' = lpos + o[i].side * (V(i).vcum - o[i].cum)
  /\ last' = <<"Unknown", "QueryFilled", "Filled">>
  /\ fresh' = FALSE
  /\ UNCHANGED <<v, ap, gh, halted, flag, riskHalted, kills, killed, pos, foreign>>

QCanceled(i) ==
  /\ o[i].st = "Unknown" /\ V(i).vst = "canceled"
  /\ o' = [o EXCEPT ![i].st = "Canceled", ![i].cum = V(i).vcum, ![i].el = 0]
  /\ lpos' = lpos + o[i].side * (V(i).vcum - o[i].cum)
  /\ last' = <<"Unknown", "QueryCanceled", "Canceled">>
  /\ fresh' = FALSE
  /\ UNCHANGED <<v, ap, gh, halted, flag, riskHalted, kills, killed, pos, foreign>>

QExpired(i) ==   \* F1: EXPIRED / EXPIRED_IN_MATCH resolves Unknown
  /\ o[i].st = "Unknown" /\ V(i).vst = "expired"
  /\ o' = [o EXCEPT ![i].st = "Expired", ![i].cum = V(i).vcum, ![i].el = 0]
  /\ lpos' = lpos + o[i].side * (V(i).vcum - o[i].cum)
  /\ last' = <<"Unknown", "QueryExpired", "Expired">>
  /\ fresh' = FALSE
  /\ UNCHANGED <<v, ap, gh, halted, flag, riskHalted, kills, killed, pos, foreign>>

\* F2/F3/F7: -2013 is definitive only when more than recvWindow + margin ticks have
\* passed since the last send of this CID. Then the intent retries under a FRESH
\* ClientOrderId (attempt + 1, unsent, approval must be fresh at the current epoch),
\* or, with attempts exhausted, is Rejected. The latch is not cleared.
QNotFound(i) ==
  /\ o[i].st = "Unknown" /\ V(i).vst = "none" /\ o[i].cum = 0
  /\ o[i].el > Bound
  /\ IF o[i].att + 1 < MaxAttempts
     THEN /\ o' = [o EXCEPT ![i].st = "PendingNew", ![i].att = @ + 1, ![i].sent = FALSE,
                     ![i].resp = "none", ![i].cxl = FALSE, ![i].cresp = "none",
                     ![i].el = 0, ![i].acked = FALSE]
          /\ last' = <<"Unknown", "QueryNotFound", "PendingNew">>
     ELSE /\ o' = [o EXCEPT ![i].st = "Rejected", ![i].sent = FALSE, ![i].resp = "none",
                     ![i].cxl = FALSE, ![i].cresp = "none"]
          /\ last' = <<"Unknown", "QueryNotFound", "Rejected">>
  /\ fresh' = FALSE
  /\ UNCHANGED <<v, ap, gh, halted, flag, riskHalted, kills, killed, lpos, pos, foreign>>

\* F3: a NotFound before the bound is not authoritative: the order stays Unknown and
\* the book stays halted.
QNotFoundEarly(i) ==
  /\ o[i].st = "Unknown" /\ V(i).vst = "none" /\ o[i].el <= Bound
  /\ last' = <<"Unknown", "QueryNotFound", "Unknown">>
  /\ UNCHANGED <<o, v, ap, gh, halted, flag, riskHalted, kills, killed, lpos, pos, foreign,
                 fresh>>

-------------------------------------------------------------------------------
\* Environment faults (no fairness)

\* A request timeout on a known nonterminal order, or a repeat on Unknown.
TimeoutEnv(i) ==
  /\ \/ o[i].st \in {"Acked", "PartiallyFilled", "PendingCancel", "Unknown"}
     \/ (o[i].st = "PendingNew" /\ o[i].sent)
  /\ o' = [o EXCEPT ![i].st = "Unknown"]
  /\ HaltG
  /\ last' = <<o[i].st, "Timeout", "Unknown">>
  /\ fresh' = FALSE
  /\ UNCHANGED <<v, riskHalted, kills, killed, lpos, pos, foreign>>

\* A Fill report with no matching venue evidence that would overfill: payload
\* validation (Overfill/MissingEvidence) rejects it before the transition.
SpurFill(i) ==
  /\ o[i].st \in States /\ Legal(o[i].st, "Fill")
  /\ HaltG
  /\ last' = <<o[i].st, "Fill", "PayloadError">>
  /\ UNCHANGED <<o, v, riskHalted, kills, killed, lpos, pos, foreign, fresh>>

RiskHalt ==
  /\ ~riskHalted
  /\ riskHalted' = TRUE /\ Bump /\ gh' = MarkHalt(gh)
  /\ UNCHANGED <<o, v, halted, flag, kills, killed, lpos, pos, foreign, fresh, last>>

Ban ==   \* venue IP/account ban (418 / -1003): halt + alert
  /\ HaltG
  /\ UNCHANGED <<o, v, riskHalted, kills, killed, lpos, pos, foreign, fresh, last>>

-------------------------------------------------------------------------------
\* Reconciliation: compare only after reports drained; never repair or adopt.

Matches == /\ ~foreign /\ lpos = pos
           /\ \A j \in Intents :
                /\ o[j].st # "Unknown"
                /\ o[j].cum = V(j).vcum
                /\ OpenLocal(j) <=> (V(j).vst = "open")
                /\ \A a \in Att : a # o[j].att => v[j][a].vst # "open"

Reconcile ==
  /\ ~fresh
  /\ \A j \in Intents : Quiet(j)
  /\ IF Matches
     THEN /\ fresh' = TRUE
          /\ UNCHANGED <<halted, flag, ap, gh>>   \* a match keeps the latch set
     ELSE /\ HaltG /\ fresh' = FALSE
  /\ UNCHANGED <<o, v, riskHalted, kills, killed, lpos, pos, foreign, last>>

OperatorReset ==
  /\ halted \/ riskHalted
  /\ fresh
  /\ \A j \in Intents : o[j].st # "Unknown"
  /\ killed => KillDone
  /\ halted' = FALSE /\ flag' = FALSE /\ riskHalted' = FALSE /\ killed' = FALSE
  /\ UNCHANGED <<o, v, ap, gh, kills, lpos, pos, foreign, fresh, last>>

-------------------------------------------------------------------------------
\* Kill: latch + epoch, cancel all, drain/reconcile, risk-approved reduce-only flatten.

Kill ==
  /\ ~killed /\ kills < MaxEpoch
  /\ killed' = TRUE /\ halted' = TRUE /\ kills' = kills + 1
  /\ Bump
  /\ gh' = [j \in Intents |-> [gh[j] EXCEPT !.ph = TRUE, !.pk = @ \/ (o[j].st \in {"PendingNew", "Unknown"})]]
  /\ UNCHANGED <<o, v, flag, riskHalted, lpos, pos, foreign, fresh, last>>

KillCancel(i) ==   \* cell CancelRequested; no approval needed
  /\ killed /\ o[i].leg # "flatten"
  /\ \/ /\ o[i].st = "PendingNew" /\ ~o[i].sent       \* F9: never sent: cancel locally
        /\ o' = [o EXCEPT ![i].st = "Canceled"]
        /\ last' = <<"PendingNew", "CancelRequested", "Canceled">>
     \/ /\ ~o[i].cxl /\ o[i].cresp = "none"
        /\ \/ o[i].st \in {"Acked", "PartiallyFilled"}
           \/ (o[i].st = "PendingNew" /\ o[i].sent)
           \/ (o[i].st = "Unknown" /\ ~o[i].cxlReq)
        /\ LET to == IF o[i].st = "Unknown" THEN "Unknown" ELSE "PendingCancel" IN
           /\ o' = [o EXCEPT ![i].st = to, ![i].cxl = TRUE, ![i].cxlReq = TRUE]
           /\ last' = <<o[i].st, "CancelRequested", to>>
  /\ fresh' = FALSE
  /\ UNCHANGED <<v, ap, gh, halted, flag, riskHalted, kills, killed, lpos, pos, foreign>>

KillForeign ==   \* cancel-all reaches venue orders the book does not know
  /\ killed /\ foreign
  /\ foreign' = FALSE /\ fresh' = FALSE
  /\ UNCHANGED <<o, v, ap, gh, halted, flag, riskHalted, kills, killed, lpos, pos, last>>

FlattenReady == killed /\ AllQuiet /\ fresh /\ lpos # 0 /\ ExposureKnown

KillFlatten(i) ==
  /\ FlattenReady /\ o[i].st = "None"
  /\ RiskApproves(TRUE, -Sign(lpos), Abs(lpos), lpos)
  /\ o' = [o EXCEPT ![i] = [EmptyO EXCEPT !.st = "PendingNew", !.qty = Abs(lpos),
              !.side = -Sign(lpos), !.ro = TRUE, !.leg = "flatten"]]
  /\ ap' = [ap EXCEPT ![i] = TRUE]
  /\ gh' = [gh EXCEPT ![i] = EmptyG]
  /\ UNCHANGED <<v, halted, flag, riskHalted, kills, killed, lpos, pos, foreign, fresh, last>>

KillStuck ==   \* flatten cannot proceed (no intent slot left in the bound): alert
  /\ FlattenReady /\ FreeIntent = {} /\ ~flag
  /\ flag' = TRUE
  /\ UNCHANGED <<o, v, ap, gh, halted, riskHalted, kills, killed, lpos, pos, foreign, fresh,
                 last>>

\* The flatten is a MARKET reduce-only order: it fills or expires.
VResolve(i) == o[i].leg = "flatten" /\ (VFill(i, o[i].att) \/ VExpire(i, o[i].att))

\* Cancelled{remaining} is emitted only after all evidenced racing fills drained.
EmitCancelled(i) ==
  /\ o[i].st \in {"Canceled", "Expired"} /\ o[i].rem = -1
  /\ o[i].cum = V(i).vcum
  /\ o' = [o EXCEPT ![i].rem = o[i].qty - o[i].cum]
  /\ UNCHANGED <<v, ap, gh, halted, flag, riskHalted, kills, killed, lpos, pos, foreign,
                 fresh, last>>

\* Explicit quiescence so TLC's deadlock check still reports any other dead end:
\* (a) halted with an alert, waiting for a human; (b) the finite intent bound used up.
Parked ==
  /\ \/ (halted /\ flag)
     \/ (FreeIntent = {} /\ AllQuiet /\ fresh)
  /\ UNCHANGED vars

-------------------------------------------------------------------------------

Next ==
  \/ \E i \in Intents :
       \/ \E t \in -MaxQty..MaxQty : Plan(i, t)
       \/ DispatchAck(i) \/ DispatchRej(i) \/ DispatchRefuse(i) \/ Reapprove(i)
       \/ Tick(i) \/ TickClock(i)
       \/ \E how \in {"landed", "inflight", "dropped"} : DispatchTimeout(i, how)
       \/ \E a \in Att : VLand(i, a) \/ VDrop(i, a) \/ VFill(i, a) \/ VExpire(i, a)
       \/ VCancel(i) \/ VPurge(i)
       \/ DAck(i) \/ DRej(i) \/ DFill(i) \/ DCresp(i) \/ DExpire(i)
       \/ QFilled(i) \/ QCanceled(i) \/ QExpired(i) \/ QNotFound(i) \/ QNotFoundEarly(i)
       \/ TimeoutEnv(i) \/ SpurFill(i)
       \/ KillCancel(i) \/ KillFlatten(i) \/ EmitCancelled(i)
  \/ RiskHalt \/ Ban \/ Reconcile \/ OperatorReset \/ Kill \/ KillForeign \/ KillStuck
  \/ Parked

Dispatch(i) == DispatchAck(i) \/ DispatchRej(i)
               \/ \E how \in {"landed", "inflight", "dropped"} : DispatchTimeout(i, how)

Fairness ==
  /\ \A i \in Intents :
       /\ WF_vars(\E a \in Att : VLand(i, a) \/ VDrop(i, a))
       /\ WF_vars(VCancel(i)) /\ WF_vars(VResolve(i))
       /\ WF_vars(DAck(i)) /\ WF_vars(DRej(i)) /\ WF_vars(DFill(i))
       /\ WF_vars(DCresp(i)) /\ WF_vars(DExpire(i))
       /\ WF_vars(QFilled(i)) /\ WF_vars(QCanceled(i)) /\ WF_vars(QExpired(i))
       /\ WF_vars(QNotFound(i)) /\ WF_vars(TickClock(i))
       /\ WF_vars(KillCancel(i)) /\ WF_vars(EmitCancelled(i))
       \* v3.1: SF, not WF. Reapprove (new in v3.1) and Tick can alternate forever, so the
       \* flatten dispatch is only intermittently enabled; SF says a flatten that is
       \* infinitely often dispatchable with a fresh approval is eventually dispatched
       \* (approval TTL > dispatch latency). Found by TLC with WF (see README, N4).
       /\ SF_vars(o[i].leg = "flatten" /\ Dispatch(i)) /\ WF_vars(DispatchRefuse(i))
  /\ WF_vars(Reconcile) /\ WF_vars(KillForeign) /\ WF_vars(KillStuck)
  /\ WF_vars(\E i \in Intents : KillFlatten(i))

Spec == Init /\ [][Next]_vars /\ Fairness

-------------------------------------------------------------------------------
\* Invariants

TypeOK ==
  /\ halted \in BOOLEAN /\ flag \in BOOLEAN /\ riskHalted \in BOOLEAN
  /\ killed \in BOOLEAN /\ foreign \in BOOLEAN /\ fresh \in BOOLEAN
  /\ kills \in 0..MaxEpoch
  /\ lpos \in Int /\ pos \in Int
  /\ ap \in [Intents -> BOOLEAN]
  /\ gh \in [Intents -> [ph : BOOLEAN, pk : BOOLEAN, ks : BOOLEAN, xs : BOOLEAN]]
  /\ \A i \in Intents :
       /\ o[i].st \in States \cup {"None"}
       /\ o[i].qty \in 0..MaxQty /\ o[i].side \in {-1, 0, 1}
       /\ o[i].ro \in BOOLEAN /\ o[i].leg \in {"none", "open", "reduce", "close", "flatten"}
       /\ o[i].cum \in Nat
       /\ o[i].sent \in BOOLEAN /\ o[i].sh \in BOOLEAN /\ o[i].acked \in BOOLEAN
       /\ o[i].aAge \in 0..(MaxAge + 1)
       /\ o[i].resp \in {"none", "ack", "rej"} /\ o[i].cresp \in {"none", "conf", "deny"}
       /\ o[i].cxl \in BOOLEAN /\ o[i].cxlReq \in BOOLEAN
       /\ o[i].rem \in -1..MaxQty
       /\ o[i].att \in Att /\ o[i].el \in 0..(Bound + 1)
       /\ \A a \in Att :
            /\ v[i][a].vst \in {"none", "open", "filled", "canceled", "expired"}
            /\ v[i][a].vcum \in Nat /\ v[i][a].vsubs \in Nat /\ v[i][a].infl \in BOOLEAN
  /\ last \in ({<<"-", "-", "-">>} \cup
               (States \X Events \X (States \cup {"IllegalV", "PayloadError"})))

\* The venue receives at most one NEW per ClientOrderId (summed over every
\* (intent, attempt) that maps to that id).
RECURSIVE SumPairs(_)
SumPairs(S) == IF S = {} THEN 0
               ELSE LET p == CHOOSE q \in S : TRUE
                    IN v[p[1]][p[2]].vsubs + SumPairs(S \ {p})
NoDuplicateSubmitPerClientOrderId ==
  \A i \in Intents : \A a \in Att :
    SumPairs({p \in Intents \X Att : CID(p[1], p[2]) = CID(i, a)}) <= 1

\* At most one order per intent live or possibly live at the venue: venue-open,
\* in flight, or the current attempt locally open or Unknown.
Live(i, a) == \/ v[i][a].vst = "open" \/ v[i][a].infl
              \/ (a = o[i].att /\ (o[i].st = "Unknown" \/ OpenLocal(i)))
NoTwoLiveVenueOrdersPerIntent ==
  \A i \in Intents : Cardinality({a \in Att : Live(i, a)}) <= 1

\* Venue fills summed over all attempts of an intent never exceed its approved qty.
FillsAcrossAttemptsWithinApproved ==
  \A i \in Intents : SumOver([a \in Att |-> v[i][a].vcum], Att) <= o[i].qty

\* No new-exposure (non-reduce-only) order is dispatched while either side is halted.
NoNewOrderAfterHalt == \A i \in Intents : (o[i].sent /\ ~o[i].ro) => ~o[i].sh

\* When kill is ready to flatten, risk approves the reduce-only flatten even halted.
ReduceOnlyFlattenAlwaysApprovable ==
  FlattenReady => RiskApproves(TRUE, -Sign(lpos), Abs(lpos), lpos)

FilledQtyNeverExceedsOrderQty ==
  \A i \in Intents : o[i].cum <= o[i].qty /\ \A a \in Att : v[i][a].vcum <= o[i].qty

CancelRemainingConsistent ==
  \A i \in Intents : o[i].rem # -1 =>
    /\ o[i].st \in {"Canceled", "Expired"}
    /\ o[i].rem = o[i].qty - o[i].cum

\* F8: no submit ever uses an approval issued before a halt (any cause).
ApprovalsNotUsedAcrossHalt == \A i \in Intents : ~gh[i].xs

\* F9: once killed, no order created before the kill is ever submitted.
NoSubmitOfPreKillOrders == \A i \in Intents : ~gh[i].ks

\* F5: Acked is only entered with a processed Ack for the current ClientOrderId.
NoGhostAcked == \A i \in Intents : o[i].st = "Acked" => o[i].acked

\* Supporting invariants
FlagImpliesHalted == flag => halted
CellsFollowMatrix ==
  last # <<"-", "-", "-">> =>
    IF last[3] = "IllegalV" THEN ~Legal(last[1], last[2])
    ELSE IF last[3] = "PayloadError" THEN Legal(last[1], last[2])
    ELSE last[3] \in Allowed(last[1], last[2])
ReduceOnlyNeverCrossesZero ==
  \A i \in Intents : (o[i].ro /\ V(i).vst = "open") =>
    /\ o[i].qty - V(i).vcum <= Abs(pos)
    /\ o[i].side * pos < 0
UnknownHalts == (\E i \in Intents : o[i].st = "Unknown") => halted
CidInjective == \A i, j \in Intents : \A a, b \in Att : CID(i, a) = CID(j, b) => i = j /\ a = b

\* Reachability recorder (not a safety property): prints each (from, event, result)
\* cell the first time it is reached, using TLC register 1. Run with -workers 1.
ASSUME TLCSet(1, {})
RecordReachedCells ==
  IF last = <<"-", "-", "-">> \/ last \in TLCGet(1) THEN TRUE
  ELSE /\ PrintT(<<"REACHED", last[1], last[2], last[3]>>)
       /\ TLCSet(1, TLCGet(1) \cup {last})

-------------------------------------------------------------------------------
\* Properties

HaltOnlyClearedByOperatorReset ==
  [][ (halted /\ ~halted') \/ (flag /\ ~flag') \/ (riskHalted /\ ~riskHalted')
        => OperatorReset ]_vars

KillEventuallyFlatOrFlagged ==
  killed ~> (KillDone \/ (halted /\ flag))

\* F1: an Unknown order whose venue order is done (expired, canceled or filled)
\* eventually leaves Unknown.
UnknownResolvesWhenVenueDone ==
  \A i \in Intents :
    (o[i].st = "Unknown" /\ V(i).vst \in {"expired", "canceled", "filled"})
      ~> (o[i].st # "Unknown")

=============================================================================
