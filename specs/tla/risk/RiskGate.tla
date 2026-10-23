------------------------------- MODULE RiskGate -------------------------------
\* Risk v0: one BTCUSDT symbol, one risk session/day, paper-only abstraction.
\* Integer quantities and prices abstract fixed-point ticks, not floating point.
\* The model starts from a reconciled, within-limit position. It is not the
\* execution startup/reconciliation machine and grants no network capability.
\*
\* Each intent id is used once. Only Check mints an ApprovedOrder. Submit takes
\* that token AND the current gate: a raw intent is never a credential. Tokens
\* bind immutable intent payloads, reserve exposure, and are consumed once.
\* Buy and sell reservations are bounded separately: opposing orders cannot
\* net away exposure, since either side may fill first or the other may cancel.
\*
\* Loss observations are current daily loss (including recovery), not increments.
\* Reaching the daily threshold latches a halt until the end of this model's
\* session. Recovery cannot clear it. Kill also latches; either first trip bumps
\* the epoch, invalidating every earlier approval. No reset/day rollover exists.
\* Following execution/OrderLifecycle.tla, a fresh, strictly reducing reduce-only
\* order may pass while halted/killed. It must never cross zero under ANY ordering
\* of reserved fills. Kill does not undo an already submitted order or fill.
\*
\* Bounds: finite ids, full fills, one approval/submit per id, constant positive
\* valuation price chosen at Init. No mark changes, partial fills, fees/funding,
\* stale snapshots, approval age, retries, persistence, multi-symbol margin, or
\* exchange I/O. The notional claim is at this fixed valuation, not future prices.
\* Those extensions need separate models; this is not an implementation proof.

EXTENDS Integers, FiniteSets

CONSTANTS Intents, MaxOrderQty, PositionLimit, NotionalLimit,
          Prices, MaxDailyLoss, MaxObservedLoss

ASSUME /\ IsFiniteSet(Intents) /\ Intents # {}
       /\ MaxOrderQty \in Nat \ {0}
       /\ PositionLimit \in Nat \ {0}
       /\ NotionalLimit \in Nat \ {0}
       /\ IsFiniteSet(Prices) /\ Prices # {} /\ Prices \subseteq Nat \ {0}
       /\ MaxDailyLoss \in Nat \ {0}
       /\ MaxObservedLoss \in Nat /\ MaxObservedLoss >= MaxDailyLoss

Abs(x) == IF x < 0 THEN -x ELSE x
Min(x, y) == IF x < y THEN x ELSE y
Max(x, y) == IF x > y THEN x ELSE y
Deltas == (-MaxOrderQty..MaxOrderQty) \ {0}
Phases == {"new", "pending", "approved", "rejected", "submitted",
           "filled", "canceled", "refused"}
Reserved == {"approved", "submitted"}
Credentials == {"raw-intent", "ApprovedOrder"}

RECURSIVE Sum(_, _)
Sum(f, S) == IF S = {} THEN 0
             ELSE LET i == CHOOSE j \in S : TRUE
                  IN f[i] + Sum(f, S \ {i})

VARIABLES phase, intent, position, price, dailyLoss, dailyHalted, killed,
          epoch, approval, receipt, submits

vars == <<phase, intent, position, price, dailyLoss, dailyHalted, killed,
          epoch, approval, receipt, submits>>

EmptyProof == [present |-> FALSE, low |-> 0, high |-> 0, halted |-> FALSE,
               killed |-> FALSE, epoch |-> 0, tokenEpoch |-> 0,
               issuer |-> "none", credential |-> "none"]
Halted == dailyHalted \/ killed

LowExcept(i) == position + Sum(
    [j \in Intents |-> IF j # i /\ phase[j] \in Reserved
                      THEN Min(intent[j].delta, 0) ELSE 0], Intents)
HighExcept(i) == position + Sum(
    [j \in Intents |-> IF j # i /\ phase[j] \in Reserved
                      THEN Max(intent[j].delta, 0) ELSE 0], Intents)
Low == position + Sum(
    [j \in Intents |-> IF phase[j] \in Reserved
                      THEN Min(intent[j].delta, 0) ELSE 0], Intents)
High == position + Sum(
    [j \in Intents |-> IF phase[j] \in Reserved
                      THEN Max(intent[j].delta, 0) ELSE 0], Intents)

PositionFits(lo, hi, d) ==
    /\ Abs(lo + Min(d, 0)) <= PositionLimit
    /\ Abs(hi + Max(d, 0)) <= PositionLimit
NotionalFits(lo, hi, d) ==
    /\ Abs(lo + Min(d, 0)) * price <= NotionalLimit
    /\ Abs(hi + Max(d, 0)) * price <= NotionalLimit
StrictReduce(lo, hi, d) ==
    \/ (d < 0 /\ lo > 0 /\ lo + d >= 0)
    \/ (d > 0 /\ hi < 0 /\ hi + d <= 0)

\* Admission uses the outstanding reservations, excluding only this intent's
\* own token on a dispatch recheck. Reduce-only is a restriction, not a label
\* that permits an increase below the normal limits.
RiskAllows(i) ==
    /\ PositionFits(LowExcept(i), HighExcept(i), intent[i].delta)
    /\ NotionalFits(LowExcept(i), HighExcept(i), intent[i].delta)
    /\ IF intent[i].reduceOnly
       THEN StrictReduce(LowExcept(i), HighExcept(i), intent[i].delta)
       ELSE ~Halted

Proof(i, credential) ==
    [present |-> TRUE, low |-> LowExcept(i), high |-> HighExcept(i),
     halted |-> Halted, killed |-> killed, epoch |-> epoch,
     tokenEpoch |-> approval[i].epoch, issuer |-> approval[i].issuer,
     credential |-> credential]

Init ==
    /\ phase = [i \in Intents |-> "new"]
    /\ intent = [i \in Intents |-> [delta |-> 0, reduceOnly |-> FALSE]]
    /\ price \in Prices
    /\ position \in -PositionLimit..PositionLimit
    /\ Abs(position) * price <= NotionalLimit
    /\ dailyLoss = 0
    /\ dailyHalted = FALSE
    /\ killed = FALSE
    /\ epoch = 0
    /\ approval = [i \in Intents |-> EmptyProof]
    /\ receipt = [i \in Intents |-> EmptyProof]
    /\ submits = [i \in Intents |-> 0]

Request(i, d, ro) ==
    /\ phase[i] = "new"
    /\ phase' = [phase EXCEPT ![i] = "pending"]
    /\ intent' = [intent EXCEPT ![i] = [delta |-> d, reduceOnly |-> ro]]
    /\ UNCHANGED <<position, price, dailyLoss, dailyHalted, killed, epoch,
                    approval, receipt, submits>>

\* The sole ApprovedOrder constructor; a veto cannot be downsized/retried.
Check(i) ==
    /\ phase[i] = "pending"
    /\ IF RiskAllows(i)
       THEN /\ phase' = [phase EXCEPT ![i] = "approved"]
            /\ approval' = [approval EXCEPT ![i] =
                 [Proof(i, "ApprovedOrder") EXCEPT !.issuer = "risk-gate",
                                                     !.tokenEpoch = epoch]]
       ELSE /\ phase' = [phase EXCEPT ![i] = "rejected"]
            /\ UNCHANGED approval
    /\ UNCHANGED <<intent, position, price, dailyLoss, dailyHalted, killed,
                    epoch, receipt, submits>>

GateAllows(i, credential) ==
    /\ credential = "ApprovedOrder"
    /\ approval[i].present /\ approval[i].issuer = "risk-gate"
    /\ approval[i].epoch = epoch
    /\ RiskAllows(i)

\* Abstract submit(ApprovedOrder, &RiskGate), with no order transport. Raw
\* attempts are admitted to Next so the gate's credential check is exercised.
\* A raw attempt leaves a legitimate held token untouched. A stale or unsafe
\* real token is consumed/refused and its reservation is released.
Submit(i, credential) ==
    /\ phase[i] \in {"pending", "approved"}
    /\ IF phase[i] = "approved" /\ GateAllows(i, credential)
       THEN /\ phase' = [phase EXCEPT ![i] = "submitted"]
            /\ receipt' = [receipt EXCEPT ![i] = Proof(i, credential)]
            /\ submits' = [submits EXCEPT ![i] = @ + 1]
       ELSE /\ IF phase[i] = "approved" /\ credential = "ApprovedOrder"
               THEN phase' = [phase EXCEPT ![i] = "refused"]
               ELSE UNCHANGED phase
            /\ UNCHANGED <<receipt, submits>>
    /\ UNCHANGED <<intent, position, price, dailyLoss, dailyHalted, killed,
                    epoch, approval>>

Fill(i) ==
    /\ phase[i] = "submitted"
    /\ phase' = [phase EXCEPT ![i] = "filled"]
    /\ position' = position + intent[i].delta
    /\ UNCHANGED <<intent, price, dailyLoss, dailyHalted, killed, epoch,
                    approval, receipt, submits>>

Cancel(i) ==
    /\ phase[i] = "submitted" \/ (killed /\ phase[i] = "approved")
    /\ phase' = [phase EXCEPT ![i] = "canceled"]
    /\ UNCHANGED <<intent, position, price, dailyLoss, dailyHalted, killed,
                    epoch, approval, receipt, submits>>

ObserveLoss(loss) ==
    /\ loss # dailyLoss
    /\ dailyLoss' = loss
    /\ dailyHalted' = (dailyHalted \/ loss >= MaxDailyLoss)
    /\ epoch' = IF ~dailyHalted /\ loss >= MaxDailyLoss
                THEN epoch + 1 ELSE epoch
    /\ UNCHANGED <<phase, intent, position, price, killed,
                    approval, receipt, submits>>

Kill ==
    /\ ~killed
    /\ killed' = TRUE
    /\ epoch' = epoch + 1
    /\ UNCHANGED <<phase, intent, position, price, dailyLoss, dailyHalted,
                    approval, receipt, submits>>

Next ==
    \/ \E i \in Intents, d \in Deltas, ro \in BOOLEAN : Request(i, d, ro)
    \/ \E i \in Intents : Check(i) \/ Fill(i) \/ Cancel(i)
    \/ \E i \in Intents, c \in Credentials : Submit(i, c)
    \/ \E loss \in 0..MaxObservedLoss : ObserveLoss(loss)
    \/ Kill

\* Fairness is a caller/environment contract, not a profitable-trading claim:
\* queued checks and held tokens eventually get serviced. During kill, pending
\* paper cancellation is eventually processed unless a fill/terminal step wins.
\* We assume neither new requests, kill activation, nor market fills are fair.
KillCancel(i) == killed /\ Cancel(i)
Spec == Init /\ [][Next]_vars
        /\ \A i \in Intents :
             WF_vars(Check(i)) /\ WF_vars(Submit(i, "ApprovedOrder"))
             /\ WF_vars(KillCancel(i))

ProofType == [present : BOOLEAN, low : (-PositionLimit)..PositionLimit,
              high : (-PositionLimit)..PositionLimit, halted : BOOLEAN,
              killed : BOOLEAN, epoch : 0..2, tokenEpoch : 0..2,
              issuer : {"none", "risk-gate"},
              credential : {"none"} \cup Credentials]
TypeOK ==
    /\ phase \in [Intents -> Phases]
    /\ intent \in [Intents -> [delta : Deltas \cup {0}, reduceOnly : BOOLEAN]]
    /\ position \in -PositionLimit..PositionLimit /\ price \in Prices
    /\ dailyLoss \in 0..MaxObservedLoss
    /\ dailyHalted \in BOOLEAN /\ killed \in BOOLEAN /\ epoch \in 0..2
    /\ approval \in [Intents -> ProofType] /\ receipt \in [Intents -> ProofType]
    /\ submits \in [Intents -> 0..2]
    /\ \A i \in Intents : phase[i] # "new" => intent[i].delta \in Deltas

PositionBound == /\ Abs(Low) <= PositionLimit /\ Abs(High) <= PositionLimit
NotionalBound == /\ Abs(Low) * price <= NotionalLimit
                 /\ Abs(High) * price <= NotionalLimit
DailyLossTripsHalt == dailyLoss >= MaxDailyLoss => dailyHalted
EpochTracksLatches ==
    epoch = (IF dailyHalted THEN 1 ELSE 0) + (IF killed THEN 1 ELSE 0)
\* Independent of the admission envelope: enumerate every possible subset of
\* outstanding fills, including filling one side while the other is canceled.
EveryFillSubsetWithinLimits ==
    \A S \in SUBSET {i \in Intents : phase[i] \in {"approved", "submitted"}} :
       LET p == position + Sum([i \in Intents |-> intent[i].delta], S)
       IN /\ Abs(p) <= PositionLimit /\ Abs(p) * price <= NotionalLimit
ReduceOnlyRemainsApprovable ==
    \A i \in Intents :
       (phase[i] = "pending" /\ intent[i].reduceOnly
        /\ StrictReduce(LowExcept(i), HighExcept(i), intent[i].delta))
       => RiskAllows(i)

SafeProof(p, i) ==
    /\ PositionFits(p.low, p.high, intent[i].delta)
    /\ NotionalFits(p.low, p.high, intent[i].delta)
    /\ (intent[i].reduceOnly => StrictReduce(p.low, p.high, intent[i].delta))
    /\ (p.halted => intent[i].reduceOnly)

ApprovalsRespectLimits ==
    \A i \in Intents : approval[i].present => SafeProof(approval[i], i)
OnlyGateIssuesApprovedOrder ==
    \A i \in Intents :
       /\ (approval[i].present => approval[i].issuer = "risk-gate")
       /\ (phase[i] \in Reserved => approval[i].present)
OnlyApprovedOrderSubmits ==
    \A i \in Intents : receipt[i].present =>
       /\ approval[i].present /\ receipt[i].issuer = "risk-gate"
       /\ receipt[i].credential = "ApprovedOrder"
       /\ receipt[i].tokenEpoch = receipt[i].epoch
DispatchRespectsCurrentGate ==
    \A i \in Intents : receipt[i].present => SafeProof(receipt[i], i)
NoIncreaseAfterKill ==
    \A i \in Intents : receipt[i].present /\ receipt[i].killed =>
        intent[i].reduceOnly
SingleUse ==
    \A i \in Intents :
       /\ submits[i] <= 1
       /\ (receipt[i].present <=> submits[i] = 1)
       /\ (phase[i] \in {"submitted", "filled"} => submits[i] = 1)

DailyHaltMonotonic == [][dailyHalted => dailyHalted']_vars
KillMonotonic == [][killed => killed']_vars
EpochMonotonic == [][epoch' >= epoch]_vars
\* The immutable proof must continue to bind the original intent payload.
IntentImmutable ==
    [][\A i \in Intents : phase[i] # "new" => intent'[i] = intent[i]]_vars
QueuedCheckTerminates ==
    \A i \in Intents : phase[i] = "pending" ~> phase[i] # "pending"
HeldTokenTerminates ==
    \A i \in Intents : phase[i] = "approved" ~> phase[i] # "approved"
\* Cancel/fill can drain orders; settled position need not become zero. The gate
\* alone cannot promise flattening, fills, or a venue response outside fairness.
KillEventuallyDrains ==
    killed ~> [](\A i \in Intents : phase[i] \notin Reserved)

=============================================================================
