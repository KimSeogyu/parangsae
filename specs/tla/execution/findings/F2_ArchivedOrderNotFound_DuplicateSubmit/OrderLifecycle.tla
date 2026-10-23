----------------------------- MODULE OrderLifecycle -----------------------------
\* Execution v0 order lifecycle, pinned to reports/execution-v0-spec.md (v3,
\* sha256 406177757097b590a8607cd9a71d0ccd2dcef06fdf2c444cc17c1bca4b549dd7).
\*
\* The local machine has exactly the 9 v3 states and the 12 v3 events. Allowed(s, e)
\* is the 108-cell matrix: a non-empty set is the legal result set of the cell, the
\* empty set is the typed IllegalTransition. Every lifecycle step in this module goes
\* through Allowed and records the cell it took in `last`.
\*
\* Around the machine: a venue per client order id, an abstract RiskGate (halt_epoch,
\* approval age), the execution halt latch and alert flag, reconciliation with no
\* adoption, zero-crossing split, and the kill sequence.
\*
\* Not modeled: prices, filters, rate limits/backoff, risk event replay seq numbers,
\* payload bytes, sim/paper. Quantities are abstract units 0..MaxQty.
\* The Rust code does not exist yet; this module is the design, not a refinement.

EXTENDS Integers, FiniteSets, TLC

CONSTANTS Intents,      \* intent ids; one slot each, ClientOrderId = <<intent, 0>>
          MaxQty,       \* max |position| and max order qty (abstract units)
          MaxAge,       \* Limits::max_approval_age in abstract ticks
          MaxEpoch,     \* bound on halt_epoch advances (kills)
          InitPos,      \* possible restored settled positions
          InitForeign   \* possible "venue has an order the book does not know"

ASSUME MaxQty \in Nat /\ MaxQty >= 1 /\ MaxAge \in Nat /\ MaxEpoch \in Nat
ASSUME InitPos \subseteq -MaxQty..MaxQty /\ InitForeign \subseteq BOOLEAN

States == {"PendingNew", "Acked", "PartiallyFilled", "Filled", "PendingCancel",
           "Canceled", "Rejected", "Expired", "Unknown"}
Events == {"Ack", "Fill", "CancelRequested", "CancelDenied", "CancelConfirmed",
           "Reject", "Expire", "Timeout", "QueryFilled", "QueryCanceled",
           "QueryNotFound", "RaceFill"}
Working  == {"PendingNew", "Acked", "PartiallyFilled", "PendingCancel"}
Terminal == {"Filled", "Canceled", "Rejected", "Expired"}

\* v3 "Venue and state machine" table, cell by cell. {} = IllegalTransition.
Allowed(s, e) ==
  CASE e = "Ack" ->
         IF s = "PendingNew" THEN {"Acked"}
         ELSE IF s \in {"Acked", "PartiallyFilled", "PendingCancel"} THEN {s} ELSE {}
    [] e = "Fill" ->
         IF s \in {"PendingNew", "Acked", "PartiallyFilled"} THEN {"PartiallyFilled", "Filled"}
         ELSE IF s = "PendingCancel" THEN {"PendingCancel", "Filled"} ELSE {}
    [] e = "CancelRequested" ->
         IF s \in Working THEN {"PendingCancel"}
         ELSE IF s = "Unknown" THEN {"Unknown"} ELSE {}
    [] e = "CancelDenied"    -> IF s = "PendingCancel" THEN {"Acked", "PartiallyFilled"} ELSE {}
    [] e = "CancelConfirmed" -> IF s = "PendingCancel" THEN {"Canceled"} ELSE {}
    [] e = "Reject"          -> IF s = "PendingNew" THEN {"Rejected"} ELSE {}
    [] e = "Expire"          -> IF s \in Working THEN {"Expired"} ELSE {}
    [] e = "Timeout"         -> IF s \in Working \cup {"Unknown"} THEN {"Unknown"} ELSE {}
    [] e = "QueryFilled"     -> IF s = "Unknown" THEN {"Filled"} ELSE {}
    [] e = "QueryCanceled"   -> IF s = "Unknown" THEN {"Canceled"} ELSE {}
    [] e = "QueryNotFound"   -> IF s = "Unknown" THEN {"PendingNew"} ELSE {}
    [] e = "RaceFill"        -> IF s = "Canceled" THEN {"Canceled"} ELSE {}

Legal(s, e) == Allowed(s, e) # {}

\* TLC prints the whole matrix once at startup; gen_transitions.py parses it.
PrintMatrix ==
  \A s \in States : \A e \in Events : PrintT(<<"CELL", s, e, Allowed(s, e)>>)
ASSUME PrintMatrix

CID(i) == <<i, 0>>    \* deterministic per (intent, attempt); v3 pins attempt = 0

Sign(x) == IF x > 0 THEN 1 ELSE IF x < 0 THEN -1 ELSE 0
Abs(x)  == IF x < 0 THEN -x ELSE x

RECURSIVE SumOver(_, _)
SumOver(f, S) == IF S = {} THEN 0
                 ELSE LET x == CHOOSE y \in S : TRUE IN f[x] + SumOver(f, S \ {x})

VARIABLES
  o,           \* [Intents -> order record]: local progress plus venue truth per CID
  halted,      \* execution halt latch
  flag,        \* operator alert raised (typed Alert)
  riskHalted,  \* risk-side halt (drawdown/kill in PortfolioSnapshot)
  epoch,       \* RiskGate halt_epoch
  killed,      \* kill sequence in progress
  lpos,        \* risk's authoritative settled position (from processed receipts)
  pos,         \* venue settled position
  foreign,     \* venue has an open order unknown to the book (never adopted)
  fresh,       \* last reconciliation matched and nothing changed since
  last         \* last lifecycle cell taken: <<from, event, result>>

vars == <<o, halted, flag, riskHalted, epoch, killed, lpos, pos, foreign, fresh, last>>

Empty == [st |-> "None", qty |-> 0, side |-> 0, ro |-> FALSE, leg |-> "none",
          cum |-> 0, sent |-> FALSE, aEp |-> 0, aAge |-> 0, sh |-> FALSE,
          resp |-> "none", cxl |-> FALSE, cresp |-> "none", cxlReq |-> FALSE,
          rem |-> -1, vst |-> "none", vcum |-> 0, vsubs |-> 0, infl |-> FALSE]
\* st    local state ("None" = slot unused; not part of the 9-state machine)
\* sent  dispatch happened and no definitive absence proof since
\* aEp, aAge  ApprovedOrder halt_epoch and age;  sh  halted when last dispatched
\* resp  submit response in flight ("ack"/"rej");  cxl  cancel request in flight
\* cresp cancel response in flight ("conf"/"deny");  rem  emitted Cancelled.remaining
\* vst   venue status ("none","open","filled","canceled","expired"); vcum venue fills
\* vsubs times the venue received a NEW for this CID;  infl  submit still in flight

Init ==
  /\ o = [i \in Intents |-> Empty]
  /\ halted = TRUE              \* fresh/restored books start halted
  /\ flag = FALSE
  /\ riskHalted = FALSE
  /\ epoch = 0
  /\ killed = FALSE
  /\ pos \in InitPos
  /\ lpos = pos
  /\ foreign \in InitForeign
  /\ fresh = FALSE
  /\ last = <<"-", "-", "-">>

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

ApprovalFresh(i) == o[i].aEp = epoch /\ o[i].aAge <= MaxAge

GateOK(i) ==
  /\ ApprovalFresh(i)
  /\ \/ (~halted /\ ~riskHalted)
     \/ (o[i].ro /\ StrictReduce(i) /\ ExposureKnown)

\* abstract risk::check for a new intent at exposure x (EL decision: a strictly
\* reducing reduce-only order is always approvable, even while halted)
RiskApproves(ro, side, qty, x) ==
  IF ro /\ Abs(x + side * qty) < Abs(x) /\ Sign(x + side * qty) * Sign(x) >= 0
  THEN TRUE
  ELSE ~halted /\ ~riskHalted

Quiet(j) == /\ o[j].resp = "none" /\ ~o[j].cxl /\ o[j].cresp = "none" /\ ~o[j].infl
            /\ (o[j].st # "Unknown" =>
                  /\ o[j].cum = o[j].vcum
                  /\ (o[j].vst = "expired" => o[j].st = "Expired"))
AllQuiet == \A j \in Intents :
              /\ o[j].st \in {"None"} \cup Terminal
              /\ Quiet(j)
              /\ (o[j].st \in {"Canceled", "Expired"} => o[j].rem # -1)
FreeIntent == {i \in Intents : o[i].st = "None"}

NoOpenOrders == /\ ~foreign
                /\ \A j \in Intents : o[j].vst # "open" /\ ~o[j].infl
                                      /\ o[j].st \notin Working \cup {"Unknown"}
KillDone == killed /\ NoOpenOrders /\ pos = 0

Halt == halted' = TRUE /\ flag' = TRUE

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
         /\ o' = [o EXCEPT ![i] = [Empty EXCEPT !.st = "PendingNew", !.qty = qty,
                     !.side = side, !.ro = ro,
                     !.leg = IF cross THEN "close" ELSE IF ro THEN "reduce" ELSE "open",
                     !.aEp = epoch]]
  /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, lpos, pos, foreign, fresh, last>>
\* Crossing zero only creates the reduce-only close leg. The open leg is a separate
\* intent created by a later Plan once the close is terminal, drained and reconciled
\* (Plan requires AllQuiet and fresh), with its own RiskApproves.

-------------------------------------------------------------------------------
\* Dispatch (submit) through the RiskGate. Retry after NotFound uses this same
\* action with the same CID and original approval.

DispatchPre(i) == o[i].st = "PendingNew" /\ ~o[i].sent /\ GateOK(i)
SentRec(i) == [o[i] EXCEPT !.sent = TRUE, !.sh = halted \/ riskHalted]

DispatchAck(i) ==
  /\ DispatchPre(i)
  /\ o' = [o EXCEPT ![i] = [SentRec(i) EXCEPT !.vst = "open", !.vsubs = @ + 1,
                                              !.resp = "ack"]]
  /\ fresh' = FALSE
  /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, lpos, pos, foreign, last>>

DispatchRej(i) ==   \* definitive venue rejection (e.g. -2022 reduce-only rejected)
  /\ DispatchPre(i)
  /\ o' = [o EXCEPT ![i] = [SentRec(i) EXCEPT !.resp = "rej"]]
  /\ fresh' = FALSE
  /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, lpos, pos, foreign, last>>

\* Timeout (-1007 "send status unknown"): the NEW may have landed, may still be in
\* flight, or may be dropped. Cell (PendingNew, Timeout).
DispatchTimeout(i, how) ==
  /\ DispatchPre(i)
  /\ how \in {"landed", "inflight", "dropped"}
  /\ o' = [o EXCEPT ![i] = [SentRec(i) EXCEPT !.st = "Unknown",
              !.vst   = IF how = "landed" THEN "open" ELSE @,
              !.vsubs = IF how = "landed" THEN @ + 1 ELSE @,
              !.infl  = (how = "inflight")]]
  /\ Halt
  /\ last' = <<"PendingNew", "Timeout", "Unknown">>
  /\ fresh' = FALSE
  /\ UNCHANGED <<riskHalted, epoch, killed, lpos, pos, foreign>>

\* Definite refusal of an unsent intent whose approval is stale/epoch-mismatched.
\* Cell (PendingNew, Reject).
DispatchRefuse(i) ==
  /\ o[i].st = "PendingNew" /\ ~o[i].sent /\ ~ApprovalFresh(i)
  /\ o' = [o EXCEPT ![i].st = "Rejected"]
  /\ last' = <<"PendingNew", "Reject", "Rejected">>
  /\ fresh' = FALSE
  /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, lpos, pos, foreign>>

Tick(i) ==   \* injected time ages an unsent approval
  /\ o[i].st = "PendingNew" /\ ~o[i].sent /\ o[i].aAge <= MaxAge
  /\ o' = [o EXCEPT ![i].aAge = @ + 1]
  /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, lpos, pos, foreign, fresh, last>>

-------------------------------------------------------------------------------
\* Venue

VLand(i) ==   \* an in-flight NEW reaches the matching engine
  /\ o[i].infl
  /\ o' = [o EXCEPT ![i].infl = FALSE, ![i].vsubs = @ + 1,
                    ![i].vst = IF @ = "none" THEN "open" ELSE @]
  /\ fresh' = FALSE
  /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, lpos, pos, foreign, last>>

VDrop(i) ==   \* the in-flight NEW is discarded (outside recvWindow, -1021)
  /\ o[i].infl
  /\ o' = [o EXCEPT ![i].infl = FALSE]
  /\ fresh' = FALSE
  /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, lpos, pos, foreign, last>>

VFill(i) ==
  /\ o[i].vst = "open" /\ o[i].vcum < o[i].qty
  /\ o' = [o EXCEPT ![i].vcum = @ + 1,
                    ![i].vst = IF o[i].vcum + 1 = o[i].qty THEN "filled" ELSE "open"]
  /\ pos' = pos + o[i].side
  /\ fresh' = FALSE
  /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, lpos, foreign, last>>

VExpire(i) ==   \* IOC/GTX/market expiry, EXPIRED or EXPIRED_IN_MATCH
  /\ o[i].vst = "open"
  /\ o' = [o EXCEPT ![i].vst = "expired"]
  /\ fresh' = FALSE
  /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, lpos, pos, foreign, last>>

VForget(i) ==   \* Query Order: CANCELED/EXPIRED, no fill, created + 3 days < now -> not found
  /\ o[i].vst \in {"canceled", "expired"} /\ o[i].vcum = 0
  /\ o' = [o EXCEPT ![i].vst = "none"]
  /\ fresh' = FALSE
  /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, lpos, pos, foreign, last>>

VCancel(i) ==
  /\ o[i].cxl
  /\ o' = [o EXCEPT ![i].cxl = FALSE,
                    ![i].vst = IF @ = "open" THEN "canceled" ELSE @,
                    ![i].cresp = IF o[i].vst = "open" THEN "conf" ELSE "deny"]
  /\ fresh' = FALSE
  /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, lpos, pos, foreign, last>>

-------------------------------------------------------------------------------
\* Venue-consistent reports into the lifecycle. A report whose cell is illegal is a
\* typed IllegalTransition: the order is unchanged, the book halts and alerts, and
\* the cell is recorded with result "IllegalV" (a real venue sequence hit it).

IllegalV(i, e, rec) ==
  /\ o' = [o EXCEPT ![i] = rec]
  /\ Halt
  /\ last' = <<o[i].st, e, "IllegalV">>
  /\ UNCHANGED <<riskHalted, epoch, killed, lpos, pos, foreign>>

DAck(i) ==
  /\ o[i].resp = "ack"
  /\ fresh' = FALSE
  /\ IF Legal(o[i].st, "Ack")
     THEN LET to == IF o[i].st = "PendingNew" THEN "Acked" ELSE o[i].st IN
          /\ o' = [o EXCEPT ![i].resp = "none", ![i].st = to]
          /\ last' = <<o[i].st, "Ack", to>>
          /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, lpos, pos, foreign>>
     ELSE IllegalV(i, "Ack", [o[i] EXCEPT !.resp = "none"])

DRej(i) ==
  /\ o[i].resp = "rej"
  /\ fresh' = FALSE
  /\ IF Legal(o[i].st, "Reject") /\ o[i].cum = 0
     THEN /\ o' = [o EXCEPT ![i].resp = "none", ![i].st = "Rejected", ![i].sent = FALSE]
          /\ last' = <<o[i].st, "Reject", "Rejected">>
          /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, lpos, pos, foreign>>
     ELSE IllegalV(i, "Reject", [o[i] EXCEPT !.resp = "none"])

\* One evidenced individual fill (exact fill id from venue). On Canceled the same
\* report is the RaceFill event. Overfill validation: cum + 1 <= qty.
DFill(i) ==
  /\ o[i].vcum > o[i].cum
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
          /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, pos, foreign>>
     ELSE IllegalV(i, e, o[i])

DCresp(i) ==
  /\ o[i].cresp # "none"
  /\ fresh' = FALSE
  /\ LET e == IF o[i].cresp = "conf" THEN "CancelConfirmed" ELSE "CancelDenied" IN
     IF Legal(o[i].st, e)
     THEN LET to == IF e = "CancelConfirmed" THEN "Canceled"
                    ELSE IF o[i].cum = 0 THEN "Acked" ELSE "PartiallyFilled" IN
          /\ o' = [o EXCEPT ![i].cresp = "none", ![i].st = to]
          /\ last' = <<o[i].st, e, to>>
          /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, lpos, pos, foreign>>
     ELSE IllegalV(i, e, [o[i] EXCEPT !.cresp = "none"])

\* Expire only after outstanding committed fills drained (cum = vcum).
DExpire(i) ==
  /\ o[i].vst = "expired" /\ o[i].cum = o[i].vcum
  /\ o[i].st \in States \ Terminal
  /\ fresh' = FALSE
  /\ IF Legal(o[i].st, "Expire")
     THEN /\ o' = [o EXCEPT ![i].st = "Expired"]
          /\ last' = <<o[i].st, "Expire", "Expired">>
          /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, lpos, pos, foreign>>
     ELSE IllegalV(i, "Expire", o[i])

\* By-ID query (origClientOrderId) of an Unknown order. Complete receipts are
\* processed once (cum := vcum).
QFilled(i) ==
  /\ o[i].st = "Unknown" /\ o[i].vst = "filled"
  /\ o' = [o EXCEPT ![i].st = "Filled", ![i].cum = o[i].vcum]
  /\ lpos' = lpos + o[i].side * (o[i].vcum - o[i].cum)
  /\ last' = <<"Unknown", "QueryFilled", "Filled">>
  /\ fresh' = FALSE
  /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, pos, foreign>>

QCanceled(i) ==
  /\ o[i].st = "Unknown" /\ o[i].vst = "canceled"
  /\ o' = [o EXCEPT ![i].st = "Canceled", ![i].cum = o[i].vcum]
  /\ lpos' = lpos + o[i].side * (o[i].vcum - o[i].cum)
  /\ last' = <<"Unknown", "QueryCanceled", "Canceled">>
  /\ fresh' = FALSE
  /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, pos, foreign>>

\* -2013 is authoritative absence only when no NEW for this CID can still land
\* (~infl) and nothing was accepted or filled. The latch is NOT cleared.
QNotFound(i) ==
  /\ o[i].st = "Unknown" /\ o[i].vst = "none" /\ ~o[i].infl /\ o[i].cum = 0
  /\ o' = [o EXCEPT ![i].st = "PendingNew", ![i].sent = FALSE, ![i].resp = "none"]
  /\ last' = <<"Unknown", "QueryNotFound", "PendingNew">>
  /\ fresh' = FALSE
  /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, lpos, pos, foreign>>
\* An Unknown order the venue reports open or expired has no v3 resolving event:
\* it stays Unknown (see README, finding F1).

-------------------------------------------------------------------------------
\* Environment faults (no fairness)

\* A request timeout on a known nonterminal order, or a repeat on Unknown.
TimeoutEnv(i) ==
  /\ \/ o[i].st \in {"Acked", "PartiallyFilled", "PendingCancel", "Unknown"}
     \/ (o[i].st = "PendingNew" /\ o[i].sent)
  /\ o' = [o EXCEPT ![i].st = "Unknown"]
  /\ Halt
  /\ last' = <<o[i].st, "Timeout", "Unknown">>
  /\ fresh' = FALSE
  /\ UNCHANGED <<riskHalted, epoch, killed, lpos, pos, foreign>>

\* A Fill report with no matching venue evidence that would overfill: payload
\* validation (Overfill/MissingEvidence) rejects it before the transition.
SpurFill(i) ==
  /\ o[i].st \in States /\ Legal(o[i].st, "Fill")
  /\ Halt
  /\ last' = <<o[i].st, "Fill", "PayloadError">>
  /\ UNCHANGED <<o, riskHalted, epoch, killed, lpos, pos, foreign, fresh>>

RiskHalt ==
  /\ ~riskHalted
  /\ riskHalted' = TRUE
  /\ UNCHANGED <<o, halted, flag, epoch, killed, lpos, pos, foreign, fresh, last>>

-------------------------------------------------------------------------------
\* Reconciliation: compare only after reports drained; never repair or adopt.

Matches == /\ ~foreign /\ lpos = pos
           /\ \A j \in Intents :
                /\ o[j].st # "Unknown"
                /\ o[j].cum = o[j].vcum
                /\ OpenLocal(j) <=> (o[j].vst = "open")

Reconcile ==
  /\ ~fresh
  /\ \A j \in Intents : Quiet(j)
  /\ IF Matches
     THEN /\ fresh' = TRUE
          /\ UNCHANGED <<halted, flag>>          \* a match keeps the latch set
     ELSE /\ Halt /\ fresh' = FALSE
  /\ UNCHANGED <<o, riskHalted, epoch, killed, lpos, pos, foreign, last>>

OperatorReset ==
  /\ halted \/ riskHalted
  /\ fresh
  /\ \A j \in Intents : o[j].st # "Unknown"
  /\ killed => KillDone
  /\ halted' = FALSE /\ flag' = FALSE /\ riskHalted' = FALSE /\ killed' = FALSE
  /\ UNCHANGED <<o, epoch, lpos, pos, foreign, fresh, last>>

-------------------------------------------------------------------------------
\* Kill: latch + epoch, cancel all, drain/reconcile, risk-approved reduce-only flatten.

Kill ==
  /\ ~killed /\ epoch < MaxEpoch
  /\ killed' = TRUE /\ halted' = TRUE /\ epoch' = epoch + 1
  /\ UNCHANGED <<o, flag, riskHalted, lpos, pos, foreign, fresh, last>>

KillCancel(i) ==   \* cell CancelRequested; no approval needed
  /\ killed /\ o[i].leg # "flatten" /\ ~o[i].cxl /\ o[i].cresp = "none"
  /\ \/ o[i].st \in {"Acked", "PartiallyFilled"}
     \/ (o[i].st = "PendingNew" /\ o[i].sent)
     \/ (o[i].st = "Unknown" /\ ~o[i].cxlReq)
  /\ LET to == IF o[i].st = "Unknown" THEN "Unknown" ELSE "PendingCancel" IN
     /\ o' = [o EXCEPT ![i].st = to, ![i].cxl = TRUE, ![i].cxlReq = TRUE]
     /\ last' = <<o[i].st, "CancelRequested", to>>
  /\ fresh' = FALSE
  /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, lpos, pos, foreign>>

KillForeign ==   \* cancel-all reaches venue orders the book does not know
  /\ killed /\ foreign
  /\ foreign' = FALSE /\ fresh' = FALSE
  /\ UNCHANGED <<o, halted, flag, riskHalted, epoch, killed, lpos, pos, last>>

FlattenReady == killed /\ AllQuiet /\ fresh /\ lpos # 0 /\ ExposureKnown

KillFlatten(i) ==
  /\ FlattenReady /\ o[i].st = "None"
  /\ RiskApproves(TRUE, -Sign(lpos), Abs(lpos), lpos)
  /\ o' = [o EXCEPT ![i] = [Empty EXCEPT !.st = "PendingNew", !.qty = Abs(lpos),
              !.side = -Sign(lpos), !.ro = TRUE, !.leg = "flatten", !.aEp = epoch]]
  /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, lpos, pos, foreign, fresh, last>>

KillStuck ==   \* flatten cannot proceed (no intent slot left in the bound): alert
  /\ FlattenReady /\ FreeIntent = {} /\ ~flag
  /\ flag' = TRUE
  /\ UNCHANGED <<o, halted, riskHalted, epoch, killed, lpos, pos, foreign, fresh, last>>

\* The flatten is a MARKET reduce-only order: it fills or expires.
VResolve(i) == o[i].leg = "flatten" /\ (VFill(i) \/ VExpire(i))

\* Cancelled{remaining} is emitted only after all evidenced racing fills drained.
EmitCancelled(i) ==
  /\ o[i].st \in {"Canceled", "Expired"} /\ o[i].rem = -1
  /\ o[i].cum = o[i].vcum
  /\ o' = [o EXCEPT ![i].rem = o[i].qty - o[i].cum]
  /\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, lpos, pos, foreign, fresh, last>>

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
       \/ DispatchAck(i) \/ DispatchRej(i) \/ DispatchRefuse(i) \/ Tick(i)
       \/ \E how \in {"landed", "inflight", "dropped"} : DispatchTimeout(i, how)
       \/ VLand(i) \/ VDrop(i) \/ VFill(i) \/ VExpire(i) \/ VCancel(i) \/ VForget(i)
       \/ DAck(i) \/ DRej(i) \/ DFill(i) \/ DCresp(i) \/ DExpire(i)
       \/ QFilled(i) \/ QCanceled(i) \/ QNotFound(i)
       \/ TimeoutEnv(i) \/ SpurFill(i)
       \/ KillCancel(i) \/ KillFlatten(i) \/ EmitCancelled(i)
  \/ RiskHalt \/ Reconcile \/ OperatorReset \/ Kill \/ KillForeign \/ KillStuck
  \/ Parked

Dispatch(i) == DispatchAck(i) \/ DispatchRej(i)
               \/ \E how \in {"landed", "inflight", "dropped"} : DispatchTimeout(i, how)

Fairness ==
  /\ \A i \in Intents :
       /\ WF_vars(VLand(i) \/ VDrop(i)) /\ WF_vars(VCancel(i)) /\ WF_vars(VResolve(i))
       /\ WF_vars(DAck(i)) /\ WF_vars(DRej(i)) /\ WF_vars(DFill(i))
       /\ WF_vars(DCresp(i)) /\ WF_vars(DExpire(i))
       /\ WF_vars(QFilled(i)) /\ WF_vars(QCanceled(i)) /\ WF_vars(QNotFound(i))
       /\ WF_vars(KillCancel(i)) /\ WF_vars(EmitCancelled(i))
       /\ WF_vars(o[i].leg = "flatten" /\ Dispatch(i)) /\ WF_vars(DispatchRefuse(i))
  /\ WF_vars(Reconcile) /\ WF_vars(KillForeign) /\ WF_vars(KillStuck)
  /\ WF_vars(\E i \in Intents : KillFlatten(i))

Spec == Init /\ [][Next]_vars /\ Fairness

-------------------------------------------------------------------------------
\* Invariants

TypeOK ==
  /\ halted \in BOOLEAN /\ flag \in BOOLEAN /\ riskHalted \in BOOLEAN
  /\ killed \in BOOLEAN /\ foreign \in BOOLEAN /\ fresh \in BOOLEAN
  /\ epoch \in 0..MaxEpoch
  /\ lpos \in Int /\ pos \in Int
  /\ \A i \in Intents :
       /\ o[i].st \in States \cup {"None"}
       /\ o[i].qty \in 0..MaxQty /\ o[i].side \in {-1, 0, 1}
       /\ o[i].ro \in BOOLEAN /\ o[i].leg \in {"none", "open", "reduce", "close", "flatten"}
       /\ o[i].cum \in Nat /\ o[i].vcum \in Nat
       /\ o[i].sent \in BOOLEAN /\ o[i].sh \in BOOLEAN
       /\ o[i].aEp \in 0..MaxEpoch /\ o[i].aAge \in 0..(MaxAge + 1)
       /\ o[i].resp \in {"none", "ack", "rej"} /\ o[i].cresp \in {"none", "conf", "deny"}
       /\ o[i].cxl \in BOOLEAN /\ o[i].cxlReq \in BOOLEAN /\ o[i].infl \in BOOLEAN
       /\ o[i].rem \in -1..MaxQty
       /\ o[i].vst \in {"none", "open", "filled", "canceled", "expired"}
       /\ o[i].vsubs \in Nat
  /\ last \in ({<<"-", "-", "-">>} \cup
               (States \X Events \X (States \cup {"IllegalV", "PayloadError"})))

\* The venue receives at most one NEW per ClientOrderId.
NoDuplicateSubmitPerClientOrderId == \A i \in Intents : o[i].vsubs <= 1

\* No new-exposure (non-reduce-only) order is dispatched while either side is halted.
NoNewOrderAfterHalt == \A i \in Intents : (o[i].sent /\ ~o[i].ro) => ~o[i].sh

\* When kill is ready to flatten, risk approves the reduce-only flatten and the gate
\* would pass it with a fresh approval, even though the book is halted.
ReduceOnlyFlattenAlwaysApprovable ==
  FlattenReady =>
    RiskApproves(TRUE, -Sign(lpos), Abs(lpos), lpos)

FilledQtyNeverExceedsOrderQty ==
  \A i \in Intents : o[i].cum <= o[i].qty /\ o[i].vcum <= o[i].qty

CancelRemainingConsistent ==
  \A i \in Intents : o[i].rem # -1 =>
    /\ o[i].st \in {"Canceled", "Expired"}
    /\ o[i].rem = o[i].qty - o[i].cum

\* Supporting invariants
FlagImpliesHalted == flag => halted
CellsFollowMatrix ==
  last # <<"-", "-", "-">> =>
    IF last[3] = "IllegalV" THEN ~Legal(last[1], last[2])
    ELSE IF last[3] = "PayloadError" THEN Legal(last[1], last[2])
    ELSE last[3] \in Allowed(last[1], last[2])
ReduceOnlyNeverCrossesZero ==
  \A i \in Intents : (o[i].ro /\ o[i].vst = "open") =>
    /\ o[i].qty - o[i].vcum <= Abs(pos)
    /\ o[i].side * pos < 0
UnknownHalts == (\E i \in Intents : o[i].st = "Unknown") => halted
CidInjective == \A i, j \in Intents : CID(i) = CID(j) => i = j

-------------------------------------------------------------------------------
\* Properties

HaltOnlyClearedByOperatorReset ==
  [][ (halted /\ ~halted') \/ (flag /\ ~flag') \/ (riskHalted /\ ~riskHalted')
        => OperatorReset ]_vars

KillEventuallyFlatOrFlagged ==
  killed ~> (KillDone \/ (halted /\ flag))

=============================================================================
