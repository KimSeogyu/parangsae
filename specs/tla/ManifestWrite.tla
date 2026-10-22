------------------------------ MODULE ManifestWrite ------------------------------
\* Idempotent fact write. One key.
\*
\* Insert stores a value. The same value leaves the slot unchanged.
\* A different value records a conflict and does not replace the stored value.
\*
\* Not modeled: checksum bytes, parquet layout, many keys, or received_ts.
\* Gap files are derived and are not this machine.
\* The Rust function is merge_fact in crates/collector/src/manifest.rs.
\* Three values still cover the absent, present, and conflict phases. The extra
\* value is what makes a second clash distinct from the first stored payload.

EXTENDS Integers

CONSTANTS Values

VARIABLES phase, stored, rejected, committed

vars == <<phase, stored, rejected, committed>>

Init ==
  /\ phase = "absent"
  /\ stored = "none"
  /\ rejected = "none"
  /\ committed = "none"

Insert(v) ==
  /\ phase = "absent"
  /\ v \in Values
  /\ phase' = "present"
  /\ stored' = v
  /\ rejected' = "none"
  /\ committed' = v

Same(v) ==
  /\ phase \in {"present", "conflict"}
  /\ v = stored
  /\ UNCHANGED vars

Clash(v) ==
  /\ phase \in {"present", "conflict"}
  /\ v \in Values
  /\ v # stored
  /\ phase' = "conflict"
  /\ rejected' = v
  /\ UNCHANGED <<stored, committed>>

Next ==
  \E v \in Values:
    \/ Insert(v)
    \/ Same(v)
    \/ Clash(v)

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ phase \in {"absent", "present", "conflict"}
  /\ (phase = "absent") <=> (stored = "none")
  /\ (phase = "absent") <=> (committed = "none")
  /\ phase = "conflict" => (rejected \in Values /\ rejected # stored)
  /\ phase = "present" => rejected = "none"
  /\ phase = "absent" => rejected = "none"
  /\ stored = "none" \/ stored \in Values
  /\ committed = "none" \/ committed \in Values

\* `committed` is written only by Insert. A clash that overwrites `stored`
\* still type-checks when `rejected` is the previous payload.
ConflictKeepsStored ==
  phase = "conflict" => stored = committed

StoredStable ==
  [][stored # "none" => stored' = stored]_vars

=============================================================================
