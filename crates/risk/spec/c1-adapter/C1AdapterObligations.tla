------------------------ MODULE C1AdapterObligations ------------------------
EXTENDS Integers

CONSTANTS BugWaitForFunding, BugStrictObservation

VARIABLES position, capturedSettlement, turn, observationCut,
          economicBeforeE, sourceBeforeE, closeRequested, cutApplied,
          drained, outcome

vars == <<position, capturedSettlement, turn, observationCut,
          economicBeforeE, sourceBeforeE, closeRequested, cutApplied,
          drained, outcome>>

\* One supported closing episode. Pending means ANY unpaid captured boundary.
\* No amount, position quantity, map identity, arming state or I/O is owned here.
Init ==
  /\ position = "Open"
  /\ capturedSettlement = "Pending"
  /\ turn = "RiskDue"
  /\ observationCut = "None"
  /\ economicBeforeE = FALSE
  /\ sourceBeforeE = FALSE
  /\ closeRequested = FALSE
  /\ cutApplied = FALSE
  /\ drained = FALSE
  /\ outcome = "Undecided"

\* This required timer turn cannot advance to boundary work without processing.
\* The old adapter returned here merely because payment was still pending.
ProcessRequiredRiskTick ==
  /\ turn = "RiskDue"
  /\ closeRequested' = ~(BugWaitForFunding /\ capturedSettlement = "Pending")
  /\ position' = IF BugWaitForFunding /\ capturedSettlement = "Pending"
                 THEN "Open" ELSE "Closing"
  /\ turn' = "Ready"
  /\ UNCHANGED <<capturedSettlement, observationCut, economicBeforeE,
                 sourceBeforeE, cutApplied, drained, outcome>>

\* These are environment classifications at E, not future-price input.
\* AfterE is a source that is still unpublished at E, not an admitted payload.
ScheduleEarlierCut ==
  /\ turn = "Ready"
  /\ turn' = "AtBoundary"
  /\ observationCut' = "BeforeE"
  /\ economicBeforeE' = TRUE
  /\ sourceBeforeE' = TRUE
  /\ UNCHANGED <<position, capturedSettlement, closeRequested, cutApplied,
                 drained, outcome>>

ScheduleEqualityCut ==
  /\ turn = "Ready"
  /\ turn' = "AtBoundary"
  /\ observationCut' = "AtE"
  /\ economicBeforeE' = TRUE
  /\ sourceBeforeE' = TRUE
  /\ UNCHANGED <<position, capturedSettlement, closeRequested, cutApplied,
                 drained, outcome>>

ScheduleUnpublishedCut ==
  /\ turn = "Ready"
  /\ turn' = "AtBoundary"
  /\ observationCut' = "AfterE"
  /\ economicBeforeE' = TRUE
  /\ sourceBeforeE' = TRUE
  /\ UNCHANGED <<position, capturedSettlement, closeRequested, cutApplied,
                 drained, outcome>>

SchedulePhysicalAtOrAfterE ==
  /\ turn = "Ready"
  /\ turn' = "AtBoundary"
  /\ observationCut' = "AtE"
  /\ economicBeforeE' = FALSE
  /\ sourceBeforeE' = TRUE
  /\ UNCHANGED <<position, capturedSettlement, closeRequested, cutApplied,
                 drained, outcome>>

ScheduleSourceAtOrAfterE ==
  /\ turn = "Ready"
  /\ turn' = "AtBoundary"
  /\ observationCut' = "AtE"
  /\ economicBeforeE' = TRUE
  /\ sourceBeforeE' = FALSE
  /\ UNCHANGED <<position, capturedSettlement, closeRequested, cutApplied,
                 drained, outcome>>

AdmittedCut ==
  economicBeforeE /\ sourceBeforeE
  /\ observationCut \in {"BeforeE", "AtE"}

\* Boundary decision is unavailable until this required drain step finishes.
DrainAlreadyAdmittedCut ==
  /\ turn = "AtBoundary"
  /\ cutApplied' = AdmittedCut
  /\ position' = IF AdmittedCut THEN "Settled" ELSE position
  /\ drained' = TRUE
  /\ turn' = "Drained"
  /\ UNCHANGED <<capturedSettlement, observationCut, economicBeforeE,
                 sourceBeforeE, closeRequested, outcome>>

\* Payment may finish independently of position lifetime, before the decision.
\* Native classification must inspect the WHOLE retained map before this event.
PayAllCaptured ==
  /\ turn # "Done"
  /\ capturedSettlement = "Pending"
  /\ capturedSettlement' = "Paid"
  /\ UNCHANGED <<position, turn, observationCut, economicBeforeE,
                 sourceBeforeE, closeRequested, cutApplied, drained, outcome>>

\* A received payment that leaves another boundary unpaid cannot clear Pending.
PayPartButStillPending ==
  /\ turn # "Done"
  /\ capturedSettlement = "Pending"
  /\ UNCHANGED vars

SettledProduct ==
  position = "Settled" /\ capturedSettlement = "Paid" /\ cutApplied

DecideAtSemanticE ==
  /\ turn = "Drained"
  /\ outcome' =
       IF SettledProduct /\ ~(BugStrictObservation /\ observationCut = "AtE")
       THEN "Closed" ELSE "Technical"
  /\ turn' = "Done"
  /\ UNCHANGED <<position, capturedSettlement, observationCut,
                 economicBeforeE, sourceBeforeE, closeRequested,
                 cutApplied, drained>>

\* The episode stops at semantic E. No late cut or payment can repair the result.
Finished ==
  /\ turn = "Done"
  /\ UNCHANGED vars

Next ==
  \/ ProcessRequiredRiskTick
  \/ ScheduleEarlierCut \/ ScheduleEqualityCut \/ ScheduleUnpublishedCut
  \/ SchedulePhysicalAtOrAfterE \/ ScheduleSourceAtOrAfterE
  \/ DrainAlreadyAdmittedCut
  \/ PayAllCaptured \/ PayPartButStillPending
  \/ DecideAtSemanticE \/ Finished

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ BugWaitForFunding \in BOOLEAN /\ BugStrictObservation \in BOOLEAN
  /\ position \in {"Open", "Closing", "Settled"}
  /\ capturedSettlement \in {"Pending", "Paid"}
  /\ turn \in {"RiskDue", "Ready", "AtBoundary", "Drained", "Done"}
  /\ observationCut \in {"None", "BeforeE", "AtE", "AfterE"}
  /\ economicBeforeE \in BOOLEAN /\ sourceBeforeE \in BOOLEAN
  /\ closeRequested \in BOOLEAN /\ cutApplied \in BOOLEAN /\ drained \in BOOLEAN
  /\ outcome \in {"Undecided", "Closed", "Technical"}

RiskTickRequestsClose ==
  turn # "RiskDue" => (closeRequested /\ position # "Open")

BoundaryDecisionFollowsDrain ==
  outcome # "Undecided" => (turn = "Done" /\ drained)

ClosedRequiresSettledProduct ==
  outcome = "Closed" => SettledProduct

LawfulEqualityCompletes ==
  (turn = "Done" /\ observationCut = "AtE" /\ SettledProduct)
  => outcome = "Closed"

UnpublishedCannotRescue ==
  observationCut = "AfterE" => ~cutApplied

PhysicalAndSourceCutExclusive ==
  cutApplied => (economicBeforeE /\ sourceBeforeE)

\* This is eligibility for native AllSettledIdle feedback, not an arming owner.
MayReportSettledIdle ==
  outcome = "Closed" /\ SettledProduct
=============================================================================
