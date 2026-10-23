------------------------------- MODULE PartitionMergeControl -------------------------------
\* Aggregate control for the generated closed partition data fold.
\* Insert, Same, and Clash are authenticated row observations only in the
\* checked v6 composition. Standalone controller inputs are not authenticated.
\* No collection, count, resource, writer, or persistence relation is modeled.

VARIABLES phase, hasInserted, hasConflict

vars == <<phase, hasInserted, hasConflict>>

Init ==
  /\ phase = "Open"
  /\ hasInserted = FALSE
  /\ hasConflict = FALSE

Insert ==
  /\ phase = "Open"
  /\ hasInserted' = TRUE
  /\ UNCHANGED <<phase, hasConflict>>

Same ==
  /\ phase = "Open"
  /\ UNCHANGED vars

Clash ==
  /\ phase = "Open"
  /\ hasConflict' = TRUE
  /\ UNCHANGED <<phase, hasInserted>>

CloseWrite ==
  /\ phase = "Open"
  /\ hasInserted /\ ~hasConflict
  /\ phase' = "ReadyWrite"
  /\ UNCHANGED <<hasInserted, hasConflict>>

CloseNoop ==
  /\ phase = "Open"
  /\ ~hasInserted /\ ~hasConflict
  /\ phase' = "ReadyNoop"
  /\ UNCHANGED <<hasInserted, hasConflict>>

CloseRefused ==
  /\ phase = "Open"
  /\ hasConflict
  /\ phase' = "ReadyRefused"
  /\ UNCHANGED <<hasInserted, hasConflict>>

ConsumeWrite ==
  /\ phase = "ReadyWrite"
  /\ phase' = "Consumed"
  /\ UNCHANGED <<hasInserted, hasConflict>>

ConsumeNoop ==
  /\ phase = "ReadyNoop"
  /\ phase' = "Consumed"
  /\ UNCHANGED <<hasInserted, hasConflict>>

ConsumeRefused ==
  /\ phase = "ReadyRefused"
  /\ phase' = "Consumed"
  /\ UNCHANGED <<hasInserted, hasConflict>>

Next ==
  \/ Insert
  \/ Same
  \/ Clash
  \/ CloseWrite
  \/ CloseNoop
  \/ CloseRefused
  \/ ConsumeWrite
  \/ ConsumeNoop
  \/ ConsumeRefused

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ phase \in {"Open", "ReadyWrite", "ReadyNoop", "ReadyRefused", "Consumed"}
  /\ hasInserted \in BOOLEAN
  /\ hasConflict \in BOOLEAN

CloseAdmission ==
  /\ (phase = "ReadyWrite" => hasInserted /\ ~hasConflict)
  /\ (phase = "ReadyNoop" => ~hasInserted /\ ~hasConflict)
  /\ (phase = "ReadyRefused" => hasConflict)

CloseChoice ==
  phase = "Open" =>
    \/ /\ ENABLED CloseWrite
       /\ ~(ENABLED CloseNoop)
       /\ ~(ENABLED CloseRefused)
    \/ /\ ~(ENABLED CloseWrite)
       /\ ENABLED CloseNoop
       /\ ~(ENABLED CloseRefused)
    \/ /\ ~(ENABLED CloseWrite)
       /\ ~(ENABLED CloseNoop)
       /\ ENABLED CloseRefused

NoInputAfterClose ==
  phase # "Open" => ~ENABLED (Insert \/ Same \/ Clash)

SingleConsumption ==
  phase = "Consumed" => ~ENABLED (ConsumeWrite \/ ConsumeNoop \/ ConsumeRefused)

ConflictSticky == [][hasConflict => hasConflict']_vars
InsertSticky == [][hasInserted => hasInserted']_vars
NoReopen == [][phase # "Open" => phase' # "Open"]_vars

=============================================================================
