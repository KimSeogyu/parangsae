#!/usr/bin/env python3
"""Generate transitions.json (v3.1) for OrderLifecycle from TLC output only.

Input: tlc.log of the pinned run (TLC 2.19, OrderLifecycle.tla/.cfg), which has
  * <<"CELL", state, event, Allowed(state, event)>> for every cell, printed by TLC
    while evaluating `ASSUME PrintMatrix` (the model's own 9 x 13 table), and
  * <<"REACHED", from, event, result>> printed by the RecordReachedCells invariant
    the first time a reachable state records that lifecycle cell result in `last`
    (result "IllegalV" = an illegal cell hit by a venue-consistent report).
The full state graph is too large for a -dump dot at the pinned constants, so
reachability comes from the REACHED lines (results only, no action names).

Outputs: transitions.json (all cells, allowed sets, v3 and v3.1-expected sets,
reached results, checks) and transitions-v3-to-v3.1.md (every cell added, removed
or changed vs v3, with its reason). V3_TABLE / V31_CHANGES below are hand
transcriptions used ONLY for comparison; they are not read from TLC.
"""
import json, re, sys, hashlib, collections

LOG = sys.argv[1] if len(sys.argv) > 1 else "tlc.log"
OUT = sys.argv[2] if len(sys.argv) > 2 else "transitions.json"
DIFF = sys.argv[3] if len(sys.argv) > 3 else "transitions-v3-to-v3.1.md"

STATES = ["PendingNew", "Acked", "PartiallyFilled", "Filled", "PendingCancel",
          "Canceled", "Rejected", "Expired", "Unknown"]
EVENTS = ["Ack", "Fill", "CancelRequested", "CancelDenied", "CancelConfirmed",
          "Reject", "Expire", "Timeout", "QueryFilled", "QueryCanceled",
          "QueryExpired", "QueryNotFound", "RaceFill"]
W = ["PendingNew", "Acked", "PartiallyFilled", "PendingCancel"]

def v3(s, e):
    """Transcription of the v3 table. Returns the set of allowed result states."""
    if e == "Ack":
        return {"Acked"} if s == "PendingNew" else ({s} if s in ("Acked", "PartiallyFilled", "PendingCancel") else set())
    if e == "Fill":   # "PendingNew/Acked/PartiallyFilled -> PartiallyFilled or Filled; PendingCancel stays unless full"
        if s in ("PendingNew", "Acked", "PartiallyFilled"): return {"PartiallyFilled", "Filled"}
        return {"PendingCancel", "Filled"} if s == "PendingCancel" else set()
    if e == "CancelRequested":  # "Working states -> PendingCancel; PendingCancel stays; Unknown stays"
        if s in W: return {"PendingCancel"}
        return {"Unknown"} if s == "Unknown" else set()
    if e == "CancelDenied": return {"Acked", "PartiallyFilled"} if s == "PendingCancel" else set()
    if e == "CancelConfirmed": return {"Canceled"} if s == "PendingCancel" else set()
    if e == "Reject": return {"Rejected"} if s == "PendingNew" else set()
    if e == "Expire": return {"Expired"} if s in W else set()          # "Known working states"
    if e == "Timeout": return {"Unknown"} if s in W + ["Unknown"] else set()  # known nonterminal; Unknown stays
    if e == "QueryFilled": return {"Filled"} if s == "Unknown" else set()
    if e == "QueryCanceled": return {"Canceled"} if s == "Unknown" else set()
    if e == "QueryNotFound": return {"PendingNew"} if s == "Unknown" else set()
    if e == "RaceFill": return {"Canceled"} if s == "Canceled" else set()
    if e == "QueryExpired": return None          # event does not exist in v3
    raise KeyError(e)
V3_TABLE = {(s, e): v3(s, e) for s in STATES for e in EVENTS}

# EL-approved v3.1 changes (hand transcription of the change request; used only to
# check that the model's matrix differs from v3 in exactly these cells).
V31_CHANGES = {
    ("Unknown", "QueryExpired"): ({"Expired"}, "F1",
        "new event: by-ID query returns EXPIRED / EXPIRED_IN_MATCH; Unknown resolves to Expired"),
    ("Unknown", "QueryNotFound"): ({"PendingNew", "Rejected", "Unknown"}, "F2/F3/F7",
        "NotFound is definitive only after elapsed > recvWindow + margin since the last send of "
        "the CID: then retry under a fresh ClientOrderId (attempt + 1) -> PendingNew, or Rejected "
        "when MaxAttempts is used up; before the bound it is not authoritative -> stays Unknown (halted)"),
    ("PendingCancel", "CancelDenied"): ({"Acked", "PartiallyFilled", "Unknown"}, "F5",
        "a deny with no processed Ack and no fill does not prove the order exists: -> Unknown "
        "(halt, query) instead of a ghost Acked"),
    ("PendingNew", "CancelRequested"): ({"PendingCancel", "Canceled"}, "F9",
        "kill on a never-sent PendingNew cancels it locally (-> Canceled, no venue request) "
        "and it is never submitted"),
}
def v31(s, e):
    if (s, e) in V31_CHANGES: return V31_CHANGES[(s, e)][0]
    if e == "QueryExpired": return set()       # new event, illegal outside Unknown
    return V3_TABLE[(s, e)]
V31_TABLE = {(s, e): v31(s, e) for s in STATES for e in EVENTS}
EXPECTED_CELLS = len(STATES) * len(EVENTS)


# ---- 1. the model's matrix and reached results, as printed by TLC
log = open(LOG, encoding="utf-8", errors="replace").read()
flat = re.sub(r"\s*\n\s*", " ", log)            # PrintT wraps long tuples over lines
cell_re = re.compile(r'<<\s*"CELL",\s*"(\w+)",\s*"(\w+)",\s*\{([^}]*)\}\s*>>')
reach_re = re.compile(r'<<\s*"REACHED",\s*"(\w+)",\s*"(\w+)",\s*"(\w+)"\s*>>')
model, seen_lines = {}, []
for m in cell_re.finditer(flat):
    s, e = m.group(1), m.group(2)
    seen_lines.append((s, e))
    model[(s, e)] = set(re.findall(r'"(\w+)"', m.group(3)))
dups = [k for k, n in collections.Counter(seen_lines).items() if n > 1]
reached = collections.defaultdict(set)
for m in reach_re.finditer(flat):
    reached[(m.group(1), m.group(2))].add(m.group(3))
completed = "Model checking completed. No error has been found." in log

# ---- 2. assemble
cells, mism31, diff = [], [], []
for s in STATES:
    for e in EVENTS:
        allowed = model.get((s, e))
        e3, e31 = V3_TABLE[(s, e)], V31_TABLE[(s, e)]
        r = sorted(reached.get((s, e), set()))
        if allowed is None: kind = "MISSING"
        elif not allowed: kind = "IllegalTransition"
        elif allowed == {s}: kind = "no-op (stay)"
        elif s in allowed: kind = "transition or stay"
        else: kind = "transition"
        reached_legal = [x for x in r if x in STATES]
        notes = []
        if "IllegalV" in r: notes.append("illegal cell hit by a venue-consistent report (halt + alert; known limit F4)")
        if allowed and not reached_legal: notes.append("legal but unreachable in the model")
        if allowed and set(reached_legal) - allowed: notes.append("reached result outside Allowed")
        if e3 is None: change = "added"
        elif allowed != e3: change = "changed"
        else: change = "unchanged"
        reason = V31_CHANGES.get((s, e), (None, "F1" if e == "QueryExpired" else None, None))
        c = {"state": s, "event": e, "kind": kind,
             "v3": sorted(e3) if e3 is not None else None,
             "v31_expected": sorted(e31),
             "model_allowed": sorted(allowed) if allowed is not None else None,
             "matches_v31_expected": allowed == e31,
             "vs_v3": change,
             "reason": reason[1] if change != "unchanged" else None,
             "reached_results": r,
             "unreached_allowed_results": sorted(allowed - set(reached_legal)) if allowed else [],
             "notes": notes}
        if allowed != e31: mism31.append((s, e))
        if change != "unchanged":
            diff.append(c)
        cells.append(c)

keys = [(c["state"], c["event"]) for c in cells]
triples = sorted({(s, e, t) for (s, e), a in model.items() for t in (a or {"IllegalTransition"})})
removed = [k for k in V3_TABLE if V3_TABLE[k] is not None and k not in model]
checks = {
    "tlc_completed_without_error": completed,
    "matrix_lines_printed_by_tlc": len(seen_lines),
    "duplicate_matrix_lines": dups,
    "states": len(STATES), "events": len(EVENTS),
    "cell_count": len(cells),
    "expected_cell_count": EXPECTED_CELLS,
    "cell_count_ok": len(cells) == EXPECTED_CELLS and len(model) == EXPECTED_CELLS,
    "cells_unique": len(set(keys)) == len(keys),
    "all_cells_match_v31_expected": not mism31,
    "mismatched_cells_vs_v31_expected": mism31,
    "legal_cells": sum(1 for c in cells if c["model_allowed"]),
    "illegal_cells": sum(1 for c in cells if c["model_allowed"] == []),
    "cells_vs_v3": {"added": sum(1 for c in diff if c["vs_v3"] == "added"),
                     "changed": sum(1 for c in diff if c["vs_v3"] == "changed"),
                     "removed": len(removed)},
    "triple_count": len(triples),
    "triples_unique": len(set(triples)) == len(triples),
    "reached_lines": sum(len(x) for x in reached.values()),
    "cells_reached": sum(1 for c in cells if c["reached_results"]),
    "illegal_cells_hit_by_venue_consistent_runs": [[c["state"], c["event"]] for c in cells if "IllegalV" in c["reached_results"]],
    "legal_cells_unreachable": [[c["state"], c["event"]] for c in cells if c["model_allowed"] and not [x for x in c["reached_results"] if x in STATES]],
    "legal_results_unreached": [[c["state"], c["event"], x] for c in cells for x in c["unreached_allowed_results"] if [y for y in c["reached_results"] if y in STATES]],
}
doc = {"model": "OrderLifecycle.tla (v3.1)",
       "sources": {"tlc_log": LOG, "tlc_log_sha256": hashlib.sha256(open(LOG, "rb").read()).hexdigest()},
       "derivation": __doc__.strip(),
       "checks": checks, "cells": cells,
       "triples": [{"state": s, "event": e, "next": t} for s, e, t in triples]}
json.dump(doc, open(OUT, "w"), indent=1)

fmt = lambda x: "—" if x is None else ("{}" if not x else "{" + ", ".join(sorted(x)) + "}")
L = ["# transitions: v3 → v3.1", "",
     f"Generated by gen_transitions.py from `{LOG}` (sha256 {doc['sources']['tlc_log_sha256']}).",
     f"v3: 9 states x 12 events = 108 cells. v3.1: {len(STATES)} states x {len(EVENTS)} events = {len(cells)} cells.",
     f"Added {checks['cells_vs_v3']['added']}, changed {checks['cells_vs_v3']['changed']}, removed {checks['cells_vs_v3']['removed']}. "
     f"Legal {checks['legal_cells']} (v3: 29), illegal {checks['illegal_cells']} (v3: 79).", "",
     "{} = IllegalTransition. Reached = results TLC recorded in a reachable state (IllegalV = illegal cell hit).", "",
     "| # | State | Event | Change | v3 | v3.1 | Reason | Reached in v3.1 model | Why |",
     "|---|---|---|---|---|---|---|---|---|"]
for n, c in enumerate(diff, 1):
    why = V31_CHANGES.get((c["state"], c["event"]), (None, None, "new event QueryExpired exists only for Unknown; illegal in every other state"))[2]
    L.append(f"| {n} | {c['state']} | {c['event']} | {c['vs_v3']} | {fmt(c['v3'])} | {fmt(c['model_allowed'])} | "
             f"{c['reason']} | {', '.join(c['reached_results']) or 'none'} | {why} |")
L += ["", "No cell was removed: all 108 v3 cells are present in v3.1; 104 of them are unchanged."
      if not removed and checks['cells_vs_v3']['changed'] == 4 else f"Removed: {removed}"]
open(DIFF, "w").write("\n".join(L) + "\n")
print(json.dumps(checks, indent=1))
ok = (checks["cell_count_ok"] and checks["cells_unique"] and not dups and checks["triples_unique"]
      and checks["all_cells_match_v31_expected"] and not removed)
sys.exit(0 if ok else 1)
