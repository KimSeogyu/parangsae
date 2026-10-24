------------------------- MODULE EvidenceLifetime -------------------------
EXTENDS Naturals, FiniteSets

(**************************************************************************)
(* PRIVATE REVIEW DRAFT. Not run by this author. Two abstract serial units. *)
(* Facts/acknowledgements must be truthful concrete observations.            *)
(* This is a safety protocol, not a filesystem, fsync, process or money model.*)
(**************************************************************************)
CONSTANT SameTrial
ASSUME SameTrial \in BOOLEAN
Units == 1..2
TrialIds == {"A", "B"}
(* Two execution/run witnesses can belong to one charged trial identity. *)
TrialOf(u) == IF u = 1 \/ SameTrial THEN "A" ELSE "B"
Outcomes == {"Complete", "Technical"}
Entry(u) == <<u, "Entered">>
Outcome(u, o) == <<u, o>>
(* Unit 0 is a sentinel for invocation facts, never an execution unit. *)
SpawnAttemptFact == <<0, "SpawnAttempted">>
SpawnFact == <<0, "Spawned">>
FactDomain == {SpawnAttemptFact, SpawnFact} \cup (Units \X ({"Entered"} \cup Outcomes))
Reports == {"Undecided", "RefusedBeforeSpawn", "SpawnUnresolved",
            "FailureAfterEntry", "ExecutionComplete"}

VARIABLES claimed, spawnAttempts, facts, factHistory,
          resident, payloadHistory, persisted,
          durableUnitStarts, unitStartHistory, chargedTrials,
          ioFailed, finalized, report
vars == <<claimed, spawnAttempts, facts, factHistory,
          resident, payloadHistory, persisted,
          durableUnitStarts, unitStartHistory, chargedTrials,
          ioFailed, finalized, report>>

HasOutcome(u) == \E o \in Outcomes : Outcome(u, o) \in facts
EntryKnown == (SpawnFact \in facts) \/ (\E u \in Units : Entry(u) \in facts)
SpawnAttemptKnown == SpawnAttemptFact \in facts
HistoricalEntry == (SpawnAttemptFact \in factHistory)
                   \/ (SpawnFact \in factHistory)
                   \/ (\E u \in Units : Entry(u) \in factHistory)

Init ==
    /\ claimed = FALSE
    /\ spawnAttempts = 0
    /\ facts = {}
    /\ factHistory = {}
    /\ resident = {}
    /\ payloadHistory = {}
    /\ persisted = {}
    /\ durableUnitStarts = {}
    /\ unitStartHistory = {}
    /\ chargedTrials = {}
    /\ ioFailed = FALSE
    /\ finalized = FALSE
    /\ report = "Undecided"

Claim ==
    /\ ~claimed /\ ~finalized
    /\ claimed' = TRUE
    /\ UNCHANGED <<spawnAttempts, facts, factHistory, resident, payloadHistory,
                    persisted, durableUnitStarts, unitStartHistory, chargedTrials,
                    ioFailed, finalized, report>>

(* Record the attempt before Popen. Failure without a returned PID cannot *)
(* establish that no child exists, and cannot authorize another attempt.  *)
AttemptSpawn ==
    /\ claimed /\ ~finalized /\ ~ioFailed /\ spawnAttempts = 0
    /\ spawnAttempts' = 1
    /\ facts' = facts \cup {SpawnAttemptFact}
    /\ factHistory' = factHistory \cup {SpawnAttemptFact}
    /\ UNCHANGED <<claimed, resident, payloadHistory, persisted,
                    durableUnitStarts, unitStartHistory, chargedTrials,
                    ioFailed, finalized, report>>

(* Wrapper binding only: successful Popen/process-identity observation. *)
Spawn ==
    /\ ~finalized /\ SpawnAttemptKnown /\ SpawnFact \notin facts
    /\ facts' = facts \cup {SpawnFact}
    /\ factHistory' = factHistory \cup {SpawnFact}
    /\ UNCHANGED <<claimed, spawnAttempts, resident, payloadHistory, persisted,
                    durableUnitStarts, unitStartHistory, chargedTrials,
                    ioFailed, finalized, report>>

(* Rust binding can Enter without Spawn: it is an in-process unit. *)
Enter(u) ==
    /\ claimed /\ ~finalized /\ ~ioFailed /\ Entry(u) \notin facts
    /\ \A v \in 1..(u-1) : Outcome(v, "Complete") \in facts
    /\ facts' = facts \cup {Entry(u)}
    /\ factHistory' = factHistory \cup {Entry(u)}
    /\ UNCHANGED <<claimed, spawnAttempts, resident, payloadHistory, persisted,
                    durableUnitStarts, unitStartHistory, chargedTrials,
                    ioFailed, finalized, report>>

(* A durable UNIT-start witness is independent of claim, Spawn and outcomes.     *)
(* Partial writes or an uncertain fsync are NOT such an acknowledgement.    *)
RecordDurableUnitStart(u) ==
    /\ claimed /\ ~finalized /\ u \notin durableUnitStarts
    /\ durableUnitStarts' = durableUnitStarts \cup {u}
    /\ unitStartHistory' = unitStartHistory \cup {u}
    /\ chargedTrials' = chargedTrials \cup {TrialOf(u)}
    /\ UNCHANGED <<claimed, spawnAttempts, facts, factHistory, resident,
                    payloadHistory, persisted, ioFailed, finalized, report>>

(* Outcome/payload ownership is established BEFORE a fallible terminal log. *)
ObserveOutcome(u, o) ==
    /\ ~finalized /\ Entry(u) \in facts /\ ~HasOutcome(u)
    /\ facts' = facts \cup {Outcome(u, o)}
    /\ factHistory' = factHistory \cup {Outcome(u, o)}
    /\ resident' = resident \cup {u}
    /\ payloadHistory' = payloadHistory \cup {u}
    /\ UNCHANGED <<claimed, spawnAttempts, persisted, durableUnitStarts,
                    unitStartHistory, chargedTrials, ioFailed, finalized, report>>

(* One channel's failure does not imply every evidence sink is unavailable. *)
IOFailure ==
    /\ claimed /\ ~finalized /\ ~ioFailed
    /\ ioFailed' = TRUE
    /\ UNCHANGED <<claimed, spawnAttempts, facts, factHistory, resident,
                    payloadHistory, persisted, durableUnitStarts, unitStartHistory,
                    chargedTrials, finalized, report>>

(* Abstract acknowledgement only; does not model actual physical durability.*)
EvidenceAcknowledged(u) ==
    /\ u \in resident /\ u \notin persisted
    /\ persisted' = persisted \cup {u}
    /\ UNCHANGED <<claimed, spawnAttempts, facts, factHistory, resident,
                    payloadHistory, durableUnitStarts, unitStartHistory,
                    chargedTrials, ioFailed, finalized, report>>

ReleaseResident(u) ==
    /\ u \in resident /\ u \in persisted
    /\ resident' = resident \ {u}
    /\ UNCHANGED <<claimed, spawnAttempts, facts, factHistory, payloadHistory,
                    persisted, durableUnitStarts, unitStartHistory, chargedTrials,
                    ioFailed, finalized, report>>

FinalizeFailure ==
    /\ claimed /\ ~finalized
    /\ ioFailed \/ (\E u \in Units : Outcome(u, "Technical") \in facts)
    /\ finalized' = TRUE
    /\ report' = IF EntryKnown THEN "FailureAfterEntry"
                 ELSE IF SpawnAttemptKnown THEN "SpawnUnresolved"
                 ELSE "RefusedBeforeSpawn"
    /\ UNCHANGED <<claimed, spawnAttempts, facts, factHistory, resident,
                    payloadHistory, persisted, durableUnitStarts, unitStartHistory,
                    chargedTrials, ioFailed>>

FinalizeComplete ==
    /\ ~finalized /\ ~ioFailed
    /\ \A u \in Units : Outcome(u, "Complete") \in facts
    /\ finalized' = TRUE
    /\ report' = "ExecutionComplete"
    /\ UNCHANGED <<claimed, spawnAttempts, facts, factHistory, resident,
                    payloadHistory, persisted, durableUnitStarts, unitStartHistory,
                    chargedTrials, ioFailed>>

(* Intentional bad fallback: a post-entry exception invents a pre-spawn     *)
(* refusal and abandons resident results. Enable ONLY in the bad control.   *)
BadFallback ==
    /\ claimed /\ ~finalized /\ ioFailed /\ (EntryKnown \/ SpawnAttemptKnown)
    /\ facts' = {}
    /\ resident' = {}
    /\ finalized' = TRUE
    /\ report' = "RefusedBeforeSpawn"
    /\ UNCHANGED <<claimed, spawnAttempts, factHistory, payloadHistory,
                    persisted, durableUnitStarts, unitStartHistory, chargedTrials,
                    ioFailed>>

Next == Claim \/ AttemptSpawn \/ Spawn \/ IOFailure \/ FinalizeFailure \/ FinalizeComplete
        \/ (\E u \in Units : Enter(u) \/ RecordDurableUnitStart(u)
             \/ EvidenceAcknowledged(u) \/ ReleaseResident(u)
             \/ (\E o \in Outcomes : ObserveOutcome(u, o)))
Spec == Init /\ [][Next]_vars
BadSpec == Init /\ [][Next \/ BadFallback]_vars

TypeOK ==
    /\ claimed \in BOOLEAN /\ spawnAttempts \in 0..1
    /\ facts \subseteq FactDomain /\ factHistory \subseteq FactDomain
    /\ resident \subseteq Units /\ payloadHistory \subseteq Units
    /\ persisted \subseteq Units
    /\ durableUnitStarts \subseteq Units /\ unitStartHistory \subseteq Units
    /\ chargedTrials \subseteq TrialIds
    /\ ioFailed \in BOOLEAN /\ finalized \in BOOLEAN /\ report \in Reports
KnownFactsRetained == factHistory \subseteq facts
KnownPayloadRetained == payloadHistory \subseteq (resident \cup persisted)
DurableUnitStartsRetained == unitStartHistory \subseteq durableUnitStarts
ChargeIsTrialImage == chargedTrials = {TrialOf(u) : u \in durableUnitStarts}
BothUnitStartWitnessesRetained ==
    unitStartHistory = Units => durableUnitStarts = Units
TwoUnitChargeCardinality ==
    durableUnitStarts = Units =>
        Cardinality(chargedTrials) = IF SameTrial THEN 1 ELSE 2
KnownEntryCannotBecomePreSpawn ==
    report = "RefusedBeforeSpawn" => ~HistoricalEntry
NoAutomaticRetry == spawnAttempts <= 1
OutcomeNotInvented ==
    \A u \in Units : ~(Outcome(u, "Complete") \in facts
                        /\ Outcome(u, "Technical") \in facts)
=============================================================================
