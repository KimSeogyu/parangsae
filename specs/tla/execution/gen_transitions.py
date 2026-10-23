#!/usr/bin/env python3
"""Generate transitions.json for OrderLifecycle from TLC output only.

Inputs (both written by TLC 2.19 on the pinned OrderLifecycle.tla/.cfg):
  tlc.log     -- TLC evaluates `ASSUME PrintMatrix`, printing one
                 <<"CELL", state, event, Allowed(state, event)>> line per cell.
                 This is the model's own 9x12 table.
  states.dot  -- `-dump dot,colorize,actionlabels`. Every node label contains
                 `last = <<from, event, result>>`, the lifecycle cell taken by the
                 step into that node. Edge labels name the TLA+ action.

Output: transitions.json with all (state, event) cells, the model's allowed set,
the v3 expected set, which outcomes the full state graph actually reached and by
which actions, plus checks: count == 108, cells unique, model == v3 per cell.
V3_TABLE below is the reference transcription of the v3 markdown table
(reports/execution-v0-spec.md, "Venue and state machine"); it is NOT read from TLC
and is used only for the comparison.
"""
import json, re, sys, hashlib, collections

LOG = sys.argv[1] if len(sys.argv) > 1 else "tlc.log"
DOT = sys.argv[2] if len(sys.argv) > 2 else "states.dot"
OUT = sys.argv[3] if len(sys.argv) > 3 else "transitions.json"

STATES = ["PendingNew", "Acked", "PartiallyFilled", "Filled", "PendingCancel",
          "Canceled", "Rejected", "Expired", "Unknown"]
EVENTS = ["Ack", "Fill", "CancelRequested", "CancelDenied", "CancelConfirmed",
          "Reject", "Expire", "Timeout", "QueryFilled", "QueryCanceled",
          "QueryNotFound", "RaceFill"]
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
    raise KeyError(e)
V3_TABLE = {(s, e): v3(s, e) for s in STATES for e in EVENTS}

# TLA+ actions that take a lifecycle cell (and set `last`); other actions keep `last`.
LIFECYCLE_ACTIONS = {"DAck", "DRej", "DFill", "DCresp", "DExpire", "QFilled", "QCanceled",
                     "QNotFound", "DispatchTimeout", "DispatchRefuse", "TimeoutEnv",
                     "SpurFill", "KillCancel"}

# ---- 1. the model's matrix, as printed by TLC
cell_re = re.compile(r'<<"CELL", "(\w+)", "(\w+)", \{([^}]*)\}>>')
model, seen_lines = {}, []
for line in open(LOG, encoding="utf-8", errors="replace"):
    m = cell_re.search(line)
    if m:
        s, e, body = m.group(1), m.group(2), m.group(3)
        seen_lines.append((s, e))
        model[(s, e)] = set(re.findall(r'"(\w+)"', body))
dups = [k for k, n in collections.Counter(seen_lines).items() if n > 1]

# ---- 2. reachability from the dot dump
node_re = re.compile(r'^(-?\d+) \[label="(.*)"')
edge_re = re.compile(r'^(-?\d+) -> (-?\d+) \[label="([^"]*)"')
last_re = re.compile(r'last = <<\\"([^\\]*)\\", \\"([^\\]*)\\", \\"([^\\]*)\\">>')
node_last, reached = {}, collections.defaultdict(lambda: collections.defaultdict(set))
nodes = edges = 0
edge_buf = []
with open(DOT, encoding="utf-8", errors="replace") as f:
    for line in f:
        m = node_re.match(line)
        if m:
            nodes += 1
            lm = last_re.search(m.group(2))
            node_last[m.group(1)] = lm.groups() if lm else None
            continue
        m = edge_re.match(line)
        if m:
            edges += 1
            edge_buf.append((m.group(1), m.group(2), m.group(3)))
for src, dst, act in edge_buf:
    a, b = node_last.get(src), node_last.get(dst)
    if b and b[0] != "-":
        # the step into dst took cell b; attribute the action when last changed
        # or when the action is a lifecycle step that re-took the same cell.
        if act in LIFECYCLE_ACTIONS:
            reached[(b[0], b[1])][b[2]].add(act)
for v in set(node_last.values()):
    if v and v[0] != "-":
        reached[(v[0], v[1])].setdefault(v[2], set())
del edge_buf

# ---- 3. assemble
cells, mismatches = [], []
for s in STATES:
    for e in EVENTS:
        allowed = model.get((s, e))
        exp = V3_TABLE[(s, e)]
        r = reached.get((s, e), {})
        if allowed is None:
            kind = "MISSING"
        elif not allowed:
            kind = "IllegalTransition"
        elif allowed == {s}:
            kind = "no-op (stay)"
        elif s in allowed:
            kind = "transition or stay"
        else:
            kind = "transition"
        reached_legal = sorted(x for x in r if x in STATES)
        findings = []
        if allowed is not None and allowed != exp:
            findings.append("model Allowed differs from v3 table")
        if "IllegalV" in r:
            findings.append("v3 rejects this cell but a venue-consistent run reaches it")
        if allowed and not reached_legal:
            findings.append("legal in v3 but unreachable in the model")
        if allowed and set(reached_legal) - allowed:
            findings.append("reached result outside Allowed")
        unreached_results = sorted(allowed - set(reached_legal)) if allowed else []
        c = {"state": s, "event": e, "kind": kind,
             "v3_expected": sorted(exp), "model_allowed": sorted(allowed) if allowed is not None else None,
             "matches_v3": allowed == exp,
             "reached_results": {k: sorted(v) for k, v in sorted(r.items())},
             "unreached_allowed_results": unreached_results,
             "findings": findings}
        if allowed != exp:
            mismatches.append((s, e))
        cells.append(c)

keys = [(c["state"], c["event"]) for c in cells]
triples = sorted({(s, e, t) for (s, e), a in model.items() for t in (a or {"IllegalTransition"})})
checks = {
    "matrix_lines_printed_by_tlc": len(seen_lines),
    "duplicate_matrix_lines": dups,
    "cell_count": len(cells),
    "cell_count_is_108": len(cells) == 108 and len(model) == 108,
    "cells_unique": len(set(keys)) == len(keys),
    "all_cells_match_v3": not mismatches,
    "mismatched_cells": mismatches,
    "legal_cells": sum(1 for c in cells if c["model_allowed"]),
    "illegal_cells": sum(1 for c in cells if c["model_allowed"] == []),
    "triple_count": len(triples),
    "triples_unique": len(set(triples)) == len(triples),
    "dot_nodes": nodes, "dot_edges": edges,
    "cells_reached_in_state_graph": sum(1 for c in cells if c["reached_results"]),
    "illegal_cells_hit_by_venue_consistent_runs": [[c["state"], c["event"]] for c in cells if "IllegalV" in c["reached_results"]],
    "legal_cells_unreachable": [[c["state"], c["event"]] for c in cells if c["model_allowed"] and not [x for x in c["reached_results"] if x in STATES]],
}
doc = {"model": "OrderLifecycle.tla",
       "sources": {"tlc_log": LOG, "dot": DOT,
                   "tlc_log_sha256": hashlib.sha256(open(LOG, "rb").read()).hexdigest()},
       "derivation": __doc__.strip(),
       "checks": checks, "cells": cells,
       "triples": [{"state": s, "event": e, "next": t} for s, e, t in triples]}
json.dump(doc, open(OUT, "w"), indent=1)
print(json.dumps(checks, indent=1))
ok = checks["cell_count_is_108"] and checks["cells_unique"] and not dups and checks["triples_unique"]
sys.exit(0 if ok else 1)
