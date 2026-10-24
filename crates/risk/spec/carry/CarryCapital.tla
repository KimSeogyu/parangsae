----------------------------- MODULE CarryCapital -----------------------------
EXTENDS Naturals, Integers, FiniteSets

CONSTANTS Target, SpotPrice, PerpPrice, SpotFee, PerpFee,
          InitialSpot, InitialPerp, IdleReserve,
          BugShortPrincipal, BugReleaseOnCancel, BugHedgeBeforeVisible

ASSUME /\ Target \in Nat \ {0}
       /\ SpotPrice \in Nat \ {0} /\ PerpPrice \in Nat \ {0}
       /\ SpotFee \in Nat /\ PerpFee \in Nat
       /\ InitialSpot >= Target * (SpotPrice + SpotFee)
       /\ InitialPerp >= Target * (PerpPrice + 2 * PerpFee)
       /\ IdleReserve \in Nat
       /\ BugShortPrincipal \in BOOLEAN
       /\ BugReleaseOnCancel \in BOOLEAN
       /\ BugHedgeBeforeVisible \in BOOLEAN

Legs == {"SpotBuy", "PerpSell", "PerpBuy", "SpotSell"}
Lives == {"Unused", "Prepared", "Submitted", "CancelPending", "Unknown", "Terminal"}

VARIABLES qty, life, dispatched, venueFilled, seenFilled, stopped, complete, held,
          spotCash, perpCash, halted, hedgeVisibleBasis, spotExitKnownShort,
          spotExitCompleteBasis

vars == <<qty, life, dispatched, venueFilled, seenFilled, stopped, complete, held,
          spotCash, perpCash, halted, hedgeVisibleBasis, spotExitKnownShort,
          spotExitCompleteBasis>>

KnownSpot == seenFilled["SpotBuy"] - seenFilled["SpotSell"]
KnownShort == seenFilled["PerpSell"] - seenFilled["PerpBuy"]
VenueSpot == venueFilled["SpotBuy"] - venueFilled["SpotSell"]
VenueShort == venueFilled["PerpSell"] - venueFilled["PerpBuy"]
SpotFees == SpotFee * (seenFilled["SpotBuy"] + seenFilled["SpotSell"])
PerpFees == PerpFee * (seenFilled["PerpSell"] + seenFilled["PerpBuy"])

Init ==
  /\ qty = [o \in Legs |-> 0]
  /\ life = [o \in Legs |-> "Unused"]
  /\ dispatched = [o \in Legs |-> FALSE]
  /\ venueFilled = [o \in Legs |-> 0]
  /\ seenFilled = [o \in Legs |-> 0]
  /\ stopped = [o \in Legs |-> FALSE]
  /\ complete = [o \in Legs |-> FALSE]
  /\ held = [o \in Legs |-> FALSE]
  /\ spotCash = InitialSpot
  /\ perpCash = InitialPerp
  /\ halted = FALSE
  /\ hedgeVisibleBasis = 0
  /\ spotExitKnownShort = 0
  /\ spotExitCompleteBasis = FALSE

Prepare(o, q) ==
  /\ ~halted /\ life[o] = "Unused"
  /\ qty' = [qty EXCEPT ![o] = q]
  /\ life' = [life EXCEPT ![o] = "Prepared"]
  /\ held' = [held EXCEPT ![o] = TRUE]
  /\ UNCHANGED <<dispatched, venueFilled, seenFilled, stopped, complete,
                 spotCash, perpCash, halted, hedgeVisibleBasis,
                 spotExitKnownShort, spotExitCompleteBasis>>

PlanSpot ==
  /\ life["PerpSell"] = "Unused" /\ life["SpotSell"] = "Unused"
  /\ Prepare("SpotBuy", Target)

PlanHedge ==
  /\ life["SpotSell"] = "Unused"
  /\ \E q \in 1..Target :
       /\ q <= (IF BugHedgeBeforeVisible
                THEN venueFilled["SpotBuy"] ELSE KnownSpot)
       /\ Prepare("PerpSell", q)

PlanPerpClose ==
  /\ complete["PerpSell"] /\ KnownShort > 0
  /\ Prepare("PerpBuy", KnownShort)

SafeToSellSpot ==
  /\ complete["SpotBuy"]
  /\ KnownShort = 0
  /\ (life["PerpSell"] = "Unused"
      \/ (complete["PerpSell"] /\ seenFilled["PerpSell"] = 0)
      \/ complete["PerpBuy"])

PlanSpotClose ==
  /\ SafeToSellSpot /\ KnownSpot > 0
  /\ Prepare("SpotSell", KnownSpot)

Dispatch(o) ==
  /\ ~halted /\ life[o] = "Prepared"
  /\ (o # "PerpSell"
      \/ qty[o] <= (IF BugHedgeBeforeVisible
                    THEN venueFilled["SpotBuy"] ELSE KnownSpot))
  /\ (o # "SpotSell" \/ SafeToSellSpot)
  /\ dispatched' = [dispatched EXCEPT ![o] = TRUE]
  /\ life' = [life EXCEPT ![o] = "Submitted"]
  /\ hedgeVisibleBasis' = IF o = "PerpSell" THEN KnownSpot ELSE hedgeVisibleBasis
  /\ spotExitKnownShort' = IF o = "SpotSell" THEN KnownShort ELSE spotExitKnownShort
  /\ spotExitCompleteBasis' = IF o = "SpotSell" THEN SafeToSellSpot ELSE spotExitCompleteBasis
  /\ UNCHANGED <<qty, venueFilled, seenFilled, stopped, complete, held,
                 spotCash, perpCash, halted>>

\* Economic fills can occur before their receipts become visible.
VenueFill(o) ==
  /\ dispatched[o] /\ ~stopped[o]
  /\ venueFilled[o] < qty[o]
  /\ venueFilled' = [venueFilled EXCEPT ![o] = @ + 1]
  /\ stopped' = [stopped EXCEPT ![o] = (@ \/ venueFilled[o] + 1 = qty[o])]
  /\ UNCHANGED <<qty, life, dispatched, seenFilled, complete, held,
                 spotCash, perpCash, halted, hedgeVisibleBasis,
                 spotExitKnownShort, spotExitCompleteBasis>>

\* One unit is an abstract fee-net spot unit or a linear-perp BTC unit.
\* Fixed prices make realized and unrealized futures P&L zero in this model.
Deliver(o) ==
  /\ seenFilled[o] < venueFilled[o]
  /\ seenFilled' = [seenFilled EXCEPT ![o] = @ + 1]
  /\ spotCash' = CASE o = "SpotBuy" -> spotCash - SpotPrice - SpotFee
                     [] o = "SpotSell" -> spotCash + SpotPrice - SpotFee
                     [] OTHER -> spotCash
  /\ perpCash' = CASE o = "PerpSell" ->
                       perpCash - PerpFee
                       + (IF BugShortPrincipal THEN PerpPrice ELSE 0)
                     [] o = "PerpBuy" -> perpCash - PerpFee
                     [] OTHER -> perpCash
  /\ UNCHANGED <<qty, life, dispatched, venueFilled, stopped, complete, held,
                 halted, hedgeVisibleBasis, spotExitKnownShort,
                 spotExitCompleteBasis>>

RequestCancel(o) ==
  /\ dispatched[o] /\ ~complete[o]
  /\ life[o] = "Submitted"
  /\ life' = [life EXCEPT ![o] = "CancelPending"]
  /\ UNCHANGED <<qty, dispatched, venueFilled, seenFilled, stopped, complete, held,
                 spotCash, perpCash, halted, hedgeVisibleBasis,
                 spotExitKnownShort, spotExitCompleteBasis>>

\* A cancel acknowledgement stops future economic fills, but earlier unseen
\* fills can still be delivered. Only CompleteCut proves the receipt drain.
CancelAck(o) ==
  /\ life[o] = "CancelPending"
  /\ life' = [life EXCEPT ![o] = "Terminal"]
  /\ stopped' = [stopped EXCEPT ![o] = TRUE]
  /\ held' = IF BugReleaseOnCancel THEN [held EXCEPT ![o] = FALSE] ELSE held
  /\ UNCHANGED <<qty, dispatched, venueFilled, seenFilled, complete,
                 spotCash, perpCash, halted, hedgeVisibleBasis,
                 spotExitKnownShort, spotExitCompleteBasis>>

Unknown(o) ==
  /\ dispatched[o] /\ ~complete[o] /\ life[o] # "Unknown"
  /\ life' = [life EXCEPT ![o] = "Unknown"]
  /\ halted' = TRUE
  /\ UNCHANGED <<qty, dispatched, venueFilled, seenFilled, stopped, complete, held,
                 spotCash, perpCash, hedgeVisibleBasis, spotExitKnownShort,
                 spotExitCompleteBasis>>

CompleteCut(o) ==
  /\ dispatched[o] /\ stopped[o] /\ ~complete[o]
  /\ seenFilled[o] = venueFilled[o]
  /\ complete' = [complete EXCEPT ![o] = TRUE]
  /\ held' = [held EXCEPT ![o] = FALSE]
  /\ life' = [life EXCEPT ![o] = "Terminal"]
  /\ UNCHANGED <<qty, dispatched, venueFilled, seenFilled, stopped,
                 spotCash, perpCash, halted, hedgeVisibleBasis,
                 spotExitKnownShort, spotExitCompleteBasis>>

\* Safety only. Quiescence is permitted; no progress/fairness claim is made.
Next ==
  \/ PlanSpot \/ PlanHedge \/ PlanPerpClose \/ PlanSpotClose
  \/ \E o \in Legs : Dispatch(o) \/ VenueFill(o) \/ Deliver(o)
                        \/ RequestCancel(o) \/ CancelAck(o) \/ Unknown(o)
                        \/ CompleteCut(o)
  \/ UNCHANGED vars

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ qty \in [Legs -> 0..Target] /\ life \in [Legs -> Lives]
  /\ dispatched \in [Legs -> BOOLEAN]
  /\ venueFilled \in [Legs -> 0..Target] /\ seenFilled \in [Legs -> 0..Target]
  /\ stopped \in [Legs -> BOOLEAN] /\ complete \in [Legs -> BOOLEAN]
  /\ held \in [Legs -> BOOLEAN] /\ halted \in BOOLEAN
  /\ spotCash \in Int /\ perpCash \in Int
  /\ hedgeVisibleBasis \in 0..Target
  /\ spotExitKnownShort \in 0..Target /\ spotExitCompleteBasis \in BOOLEAN
  /\ \A o \in Legs : seenFilled[o] <= venueFilled[o] /\ venueFilled[o] <= qty[o]

NoShortPrincipalCash == perpCash = InitialPerp - PerpFees

NeverReleaseUnreconciledReservation ==
  \A o \in Legs : (dispatched[o] /\ ~complete[o]) => held[o]

HedgeOnlyAfterVisibleFirstLegReceipt ==
  dispatched["PerpSell"] =>
    (hedgeVisibleBasis > 0 /\ qty["PerpSell"] <= hedgeVisibleBasis)

OneCapitalPool ==
  spotCash + perpCash + IdleReserve + KnownSpot * SpotPrice
    = InitialSpot + InitialPerp + IdleReserve - SpotFees - PerpFees

NoNegativeCashOrNakedShort ==
  /\ spotCash >= 0 /\ perpCash >= 0
  /\ KnownSpot >= 0 /\ KnownShort >= 0
  /\ VenueSpot >= 0 /\ VenueShort >= 0 /\ VenueShort <= VenueSpot

SpotExitAfterKnownPerpFlat ==
  dispatched["SpotSell"] => (spotExitCompleteBasis /\ spotExitKnownShort = 0)

\* Expected to fail only in a separate reachability check. This is a goal,
\* not a safety promise: show that the complete paired cycle is reachable.
FullCycleUnreachable ==
  ~( /\ \A o \in Legs : complete[o]
     /\ seenFilled["SpotBuy"] = Target
     /\ seenFilled["PerpSell"] = Target
     /\ KnownSpot = 0 /\ KnownShort = 0 )
=============================================================================
