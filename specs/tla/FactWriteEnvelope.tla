--------------------------- MODULE FactWriteEnvelope ---------------------------
\* One immutable payload and its associated receipt/source provenance.
\* Values and Receipts are separate opaque equality domains.
\* The first payload keeps its first provenance. A conflicting write replaces
\* the latest rejected payload and provenance together. An equal stored payload
\* leaves every modeled variable unchanged.
\*
\* Provenance is an immutable identity, not a timestamp ordering or file format.
\* An application adapter must bind complete owned facts to both identities.
\* Partition commits, persistence, and concurrent writers are outside this model.

EXTENDS Integers

CONSTANTS Values, Receipts

VARIABLES phase, stored, rejected, storedProvenance, rejectedProvenance

vars == <<phase, stored, rejected, storedProvenance, rejectedProvenance>>

Init ==
  /\ phase = "absent"
  /\ stored = "none"
  /\ rejected = "none"
  /\ storedProvenance = "none"
  /\ rejectedProvenance = "none"

Insert(v, provenance) ==
  /\ phase = "absent"
  /\ v \in Values
  /\ provenance \in Receipts
  /\ phase' = "present"
  /\ stored' = v
  /\ storedProvenance' = provenance
  /\ rejected' = "none"
  /\ rejectedProvenance' = "none"

Same(v, provenance) ==
  /\ phase \in {"present", "conflict"}
  /\ v = stored
  /\ provenance \in Receipts
  /\ UNCHANGED vars

Clash(v, provenance) ==
  /\ phase \in {"present", "conflict"}
  /\ v \in Values
  /\ v # stored
  /\ provenance \in Receipts
  /\ phase' = "conflict"
  /\ rejected' = v
  /\ rejectedProvenance' = provenance
  /\ UNCHANGED <<stored, storedProvenance>>

Next ==
  \E v \in Values, provenance \in Receipts:
    \/ Insert(v, provenance)
    \/ Same(v, provenance)
    \/ Clash(v, provenance)

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ phase \in {"absent", "present", "conflict"}
  /\ (phase = "absent") <=> (stored = "none")
  /\ phase = "conflict" => (rejected \in Values /\ rejected # stored)
  /\ phase = "present" => rejected = "none"
  /\ phase = "absent" => rejected = "none"
  /\ stored = "none" \/ stored \in Values
  /\ (phase = "absent") <=> (storedProvenance = "none")
  /\ phase # "absent" => storedProvenance \in Receipts
  /\ phase = "conflict" => rejectedProvenance \in Receipts
  /\ phase # "conflict" => rejectedProvenance = "none"

StoredStable ==
  [][stored # "none" => stored' = stored]_vars

ProvenanceStable ==
  [][stored # "none" => storedProvenance' = storedProvenance]_vars

=============================================================================
