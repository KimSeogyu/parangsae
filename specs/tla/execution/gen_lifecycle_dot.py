#!/usr/bin/env python3
"""Abstracted lifecycle graph from transitions.json (itself generated from TLC output).
Nodes: the 9 v3 states plus one IllegalTransition sink. Edges, one per (cell, result):
  solid black  = legal result reached in the full TLC state graph
  dashed grey  = legal in Allowed but not reached under the pinned constants
  red          = cell v3 marks illegal, reached by a venue-consistent report sequence
Unreached illegal cells (the rest of the 79) are not drawn."""
import json, sys
src = sys.argv[1] if len(sys.argv) > 1 else "transitions.json"
out = sys.argv[2] if len(sys.argv) > 2 else "lifecycle.dot"
d = json.load(open(src))
L = ['digraph OrderLifecycleCells {', ' rankdir=LR; node [shape=box, fontname="Helvetica"]; edge [fontname="Helvetica", fontsize=9];',
     ' "IllegalTransition" [label="IllegalTransition\\n(order unchanged; halt + alert)", color=red, fontcolor=red];']
for c in d["cells"]:
    s, e, r = c["state"], c["event"], c["reached_results"]
    for t in c["model_allowed"] or []:
        if t in r:
            L.append(f' "{s}" -> "{t}" [label="{e}"];')
        else:
            L.append(f' "{s}" -> "{t}" [label="{e} (unreached)", style=dashed, color=grey, fontcolor=grey];')
    if "IllegalV" in r:
        L.append(f' "{s}" -> "IllegalTransition" [label="{e}", color=red, fontcolor=red];')
L.append("}")
open(out, "w").write("\n".join(L) + "\n")
print(f"{out}: {len(L) - 4} edges")
