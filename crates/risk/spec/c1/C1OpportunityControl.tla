------------------------ MODULE C1OpportunityControl ------------------------
EXTENDS Integers

CONSTANTS OmitDisarm, AllowStoppedRestart

VARIABLES mode, armed, evaluation, settledIdle, fundingPending,
          healthy, epochPending, stoppedEver

vars == <<mode, armed, evaluation, settledIdle, fundingPending,
          healthy, epochPending, stoppedEver>>

Init ==
  /\ mode = "Idle"
  /\ armed = FALSE
  /\ evaluation = "Consumed"
  /\ settledIdle = TRUE
  /\ fundingPending = FALSE
  /\ healthy = TRUE
  /\ epochPending = FALSE
  /\ stoppedEver = FALSE

FreshIdle ==
  /\ mode \in {"Idle", "Disabled"}
  /\ healthy /\ settledIdle /\ ~fundingPending /\ ~epochPending
  /\ evaluation' = "Pending"
  /\ UNCHANGED <<mode, armed, settledIdle, fundingPending,
                 healthy, epochPending, stoppedEver>>

FreshOccupied ==
  /\ mode \in {"Reserved", "Busy"}
  /\ evaluation' = "Consumed"
  /\ UNCHANGED <<mode, armed, settledIdle, fundingPending,
                 healthy, epochPending, stoppedEver>>

Replay ==
  /\ evaluation = "Consumed"
  /\ UNCHANGED vars

EvaluateFalse ==
  /\ mode = "Idle" /\ healthy /\ settledIdle
  /\ ~fundingPending /\ ~epochPending /\ evaluation = "Pending"
  /\ armed' = TRUE
  /\ evaluation' = "Consumed"
  /\ UNCHANGED <<mode, settledIdle, fundingPending,
                 healthy, epochPending, stoppedEver>>

EvaluateEligible ==
  /\ mode = "Idle" /\ healthy /\ settledIdle
  /\ ~fundingPending /\ ~epochPending /\ evaluation = "Pending"
  /\ mode' = IF armed THEN "Reserved" ELSE "Idle"
  /\ settledIdle' = IF armed THEN FALSE ELSE TRUE
  /\ armed' = IF OmitDisarm THEN armed ELSE FALSE
  /\ evaluation' = "Consumed"
  /\ UNCHANGED <<fundingPending, healthy, epochPending, stoppedEver>>

EvaluateNoSize ==
  /\ mode = "Idle" /\ healthy /\ settledIdle
  /\ ~fundingPending /\ ~epochPending /\ evaluation = "Pending"
  /\ evaluation' = "Consumed"
  /\ UNCHANGED <<mode, armed, settledIdle, fundingPending,
                 healthy, epochPending, stoppedEver>>

DisableIdle ==
  /\ mode = "Idle" /\ healthy /\ settledIdle /\ ~fundingPending
  /\ mode' = "Disabled"
  /\ UNCHANGED <<armed, evaluation, settledIdle, fundingPending,
                 healthy, epochPending, stoppedEver>>

EnableIdle ==
  /\ mode = "Disabled" /\ healthy /\ settledIdle /\ ~fundingPending
  /\ mode' = "Idle"
  /\ UNCHANGED <<armed, evaluation, settledIdle, fundingPending,
                 healthy, epochPending, stoppedEver>>

IntentAccepted ==
  /\ mode = "Reserved"
  /\ mode' = "Busy"
  /\ UNCHANGED <<armed, evaluation, settledIdle, fundingPending,
                 healthy, epochPending, stoppedEver>>

IntentRefused ==
  /\ mode = "Reserved" /\ ~fundingPending
  /\ mode' = "Idle"
  /\ settledIdle' = TRUE
  /\ UNCHANGED <<armed, evaluation, fundingPending,
                 healthy, epochPending, stoppedEver>>

RecordPendingFunding ==
  /\ mode \in {"Busy", "Stopped"}
  /\ fundingPending' = TRUE
  /\ settledIdle' = FALSE
  /\ UNCHANGED <<mode, armed, evaluation, healthy, epochPending, stoppedEver>>

RecordFundingSettled ==
  /\ mode \in {"Busy", "Stopped"} /\ fundingPending
  /\ fundingPending' = FALSE
  /\ UNCHANGED <<mode, armed, evaluation, settledIdle,
                 healthy, epochPending, stoppedEver>>

AllSettledIdle ==
  /\ mode \in {"Busy", "Stopped"} /\ ~fundingPending
  /\ mode' = IF mode = "Stopped" THEN "Stopped" ELSE "Idle"
  /\ settledIdle' = TRUE
  /\ armed' = FALSE
  /\ evaluation' = "Consumed"
  /\ UNCHANGED <<fundingPending, healthy, epochPending, stoppedEver>>

RequestEpoch ==
  /\ mode # "Stopped" /\ healthy
  /\ epochPending' = TRUE
  /\ UNCHANGED <<mode, armed, evaluation, settledIdle, fundingPending,
                 healthy, stoppedEver>>

NewEpoch ==
  /\ mode \in {"Idle", "Disabled"}
  /\ healthy /\ settledIdle /\ ~fundingPending /\ epochPending
  /\ armed' = FALSE
  /\ evaluation' = "Consumed"
  /\ epochPending' = FALSE
  /\ UNCHANGED <<mode, settledIdle, fundingPending, healthy, stoppedEver>>

Stop ==
  /\ mode' = "Stopped"
  /\ armed' = FALSE
  /\ healthy' = FALSE
  /\ evaluation' = "Consumed"
  /\ epochPending' = FALSE
  /\ stoppedEver' = TRUE
  /\ UNCHANGED <<settledIdle, fundingPending>>

StoppedWait ==
  /\ mode = "Stopped"
  /\ UNCHANGED vars

RestartStopped ==
  /\ AllowStoppedRestart /\ mode = "Stopped"
  /\ mode' = "Idle"
  /\ armed' = FALSE
  /\ evaluation' = "Consumed"
  /\ settledIdle' = TRUE
  /\ fundingPending' = FALSE
  /\ healthy' = TRUE
  /\ epochPending' = FALSE
  /\ UNCHANGED stoppedEver

Next ==
  \/ FreshIdle \/ FreshOccupied \/ Replay
  \/ EvaluateFalse \/ EvaluateEligible \/ EvaluateNoSize
  \/ DisableIdle \/ EnableIdle \/ IntentAccepted \/ IntentRefused
  \/ RecordPendingFunding \/ RecordFundingSettled \/ AllSettledIdle
  \/ RequestEpoch \/ NewEpoch \/ Stop \/ StoppedWait \/ RestartStopped

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ mode \in {"Idle", "Reserved", "Busy", "Disabled", "Stopped"}
  /\ armed \in BOOLEAN
  /\ evaluation \in {"Pending", "Consumed"}
  /\ settledIdle \in BOOLEAN /\ fundingPending \in BOOLEAN
  /\ healthy \in BOOLEAN /\ epochPending \in BOOLEAN
  /\ stoppedEver \in BOOLEAN

ReservedBusyDisarmed ==
  mode \in {"Reserved", "Busy"} => ~armed

StoppedCannotRestart ==
  stoppedEver => mode = "Stopped"

IdleHasNoLiability ==
  mode \in {"Idle", "Disabled"} => (settledIdle /\ ~fundingPending /\ healthy)

BusyHasNotBeenDeclaredFlat ==
  mode \in {"Reserved", "Busy"} => ~settledIdle

ArmedOnlyInIdleControl ==
  armed => mode \in {"Idle", "Disabled"}

ReservedConsumesEvaluation ==
  mode = "Reserved" => evaluation = "Consumed"

StoppedHasNoEntryPermission ==
  mode = "Stopped" => (~armed /\ ~healthy /\ evaluation = "Consumed")
=============================================================================
