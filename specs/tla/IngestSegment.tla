----------------------------- MODULE IngestSegment -----------------------------
\* Sequenced ingest segment. One feed, update ids in 1..Max.
\*
\* Accept a contiguous id (u = last_u + 1).
\* A duplicate id (u <= last_u) leaves the segment unchanged.
\* A jump (u > last_u + 1) ends the segment: phase becomes "gap" and no later
\* update is appended until Resync. Resync starts a new empty segment.
\*
\* Not modeled: bytes on the wire, checksums, or multiple symbols.
\* The transitions are implemented in crates/collector/src/segment.rs. A proptest
\* replays enabled actions and checks Contiguous, GapHoldsSnapshot, and
\* NoAppendWhileGap after every step. StoredStable is the ManifestWrite property;
\* merge_fact's trace check covers it.
\* With a finite Max, an open segment at last_u = Max has only duplicate steps left.
\* Those steps do not change state. That is the bound, not a gap.

EXTENDS Integers, Sequences

CONSTANT Max
ASSUME Max \in Nat /\ Max >= 2

Ids == 1..Max

VARIABLES phase, last_u, accepted, accepted_count, gap_snapshot

vars == <<phase, last_u, accepted, accepted_count, gap_snapshot>>

Init ==
  /\ phase = "open"
  /\ last_u = 0
  /\ accepted = <<>>
  /\ accepted_count = 0
  /\ gap_snapshot = <<>>

\* Idempotent replay of an id already covered by this segment.
Duplicate(u) ==
  /\ phase = "open"
  /\ u \in Ids
  /\ u <= last_u
  /\ UNCHANGED vars

Accept(u) ==
  /\ phase = "open"
  /\ u \in Ids
  /\ u = last_u + 1
  /\ last_u' = u
  /\ accepted' = Append(accepted, u)
  /\ accepted_count' = accepted_count + 1
  /\ UNCHANGED <<phase, gap_snapshot>>

\* Sequence break. The segment freezes; the jumped id is not stored.
Break(u) ==
  /\ phase = "open"
  /\ u \in Ids
  /\ u > last_u + 1
  /\ phase' = "gap"
  /\ gap_snapshot' = accepted
  /\ UNCHANGED <<last_u, accepted, accepted_count>>

\* While the segment is broken, updates are ignored.
RejectWhileGap(u) ==
  /\ phase = "gap"
  /\ u \in Ids
  /\ UNCHANGED vars

\* A fresh snapshot opens a new segment. It does not backfill the gap.
Resync(snapshot) ==
  /\ phase = "gap"
  /\ snapshot \in 0..Max
  /\ phase' = "open"
  /\ last_u' = snapshot
  /\ accepted' = <<>>
  /\ accepted_count' = 0
  /\ gap_snapshot' = <<>>

Next ==
  \/ \E u \in Ids : Duplicate(u) \/ Accept(u) \/ Break(u) \/ RejectWhileGap(u)
  \/ \E snapshot \in 0..Max : Resync(snapshot)

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ phase \in {"open", "gap"}
  /\ last_u \in 0..Max
  /\ accepted \in Seq(Ids)
  /\ gap_snapshot \in Seq(Ids)
  /\ accepted_count \in 0..Max
  /\ Len(accepted) = accepted_count

\* Accepted ids step by one. A hole inside the segment is a bug.
Contiguous ==
  \A i \in 1..(Len(accepted) - 1) : accepted[i + 1] = accepted[i] + 1

NoRepeatedId ==
  \A i, j \in 1..Len(accepted) : i # j => accepted[i] # accepted[j]

\* The frozen copy taken at the break still equals the segment.
GapHoldsSnapshot ==
  phase = "gap" => accepted = gap_snapshot

\* A step that stays in "gap" does not append. Resync leaves "gap", so it may clear.
NoAppendWhileGap ==
  [][(phase = "gap" /\ phase' = "gap") => accepted' = accepted]_vars

=============================================================================
