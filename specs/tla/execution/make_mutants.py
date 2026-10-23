#!/usr/bin/env python3
"""Build mutants/ and findings/ variants of OrderLifecycle.tla (v3.1) by exact text
replacement (each `old` must match exactly once), then run TLC on each with the CI
command shape. Constants are copied from OrderLifecycle.cfg; each variant's .cfg has
TypeOK plus its target only. Usage: python3 make_mutants.py /path/to/tla2tools.jar"""
import os, subprocess, sys, difflib, shutil, re
JAR = sys.argv[1]
BASE = open("OrderLifecycle.tla").read()
CONSTS = "SPECIFICATION Spec\n" + "".join(
    l for l in open("OrderLifecycle.cfg") if l.startswith("CONSTANT")) + "\n"

QEXP_OLD = '''QExpired(i) ==   \\* F1: EXPIRED / EXPIRED_IN_MATCH resolves Unknown
  /\\ o[i].st = "Unknown" /\\ V(i).vst = "expired"
  /\\ o' = [o EXCEPT ![i].st = "Expired", ![i].cum = V(i).vcum, ![i].el = 0]
  /\\ lpos' = lpos + o[i].side * (V(i).vcum - o[i].cum)
  /\\ last' = <<"Unknown", "QueryExpired", "Expired">>
'''
VARIANTS = [
 # (group, name, why, [(old, new), ...], checks)
 ("mutants", "M1_NoDuplicateSubmitPerClientOrderId",
  "ClientOrderId ignores the attempt number: a retry reuses the id of an order the venue already received",
  [("CID(i, a) == <<i, a>>  ", "CID(i, a) == <<i, 0>>  ")],
  "INVARIANT TypeOK\nINVARIANT NoDuplicateSubmitPerClientOrderId\n"),
 ("mutants", "M2_NoNewOrderAfterHalt",
  "dispatch gate ignores the halt latch AND halts do not bump halt_epoch (the gate-only mutant is masked by F8: see S1)",
  [("  /\\ \\/ (~halted /\\ ~riskHalted)\n     \\/ (o[i].ro /\\ StrictReduce(i) /\\ ExposureKnown)\n",
    "  /\\ \\/ TRUE\n     \\/ (o[i].ro /\\ StrictReduce(i) /\\ ExposureKnown)\n"),
   ("Bump == ap' = [j \\in Intents |-> FALSE]", "Bump == UNCHANGED ap")],
  "INVARIANT TypeOK\nINVARIANT NoNewOrderAfterHalt\n"),
 ("findings", "S1_GateOnlyMaskedByEpoch",
  "dispatch gate ignores the halt latch, epoch bump intact. EXPECTED TO SURVIVE: every halt invalidates pre-halt approvals (F8) and risk::check refuses non-reduce re-approval while halted, so no non-reduce order is sent after a halt even without the gate (defense in depth)",
  [("  /\\ \\/ (~halted /\\ ~riskHalted)\n     \\/ (o[i].ro /\\ StrictReduce(i) /\\ ExposureKnown)\n",
    "  /\\ \\/ TRUE\n     \\/ (o[i].ro /\\ StrictReduce(i) /\\ ExposureKnown)\n")],
  "INVARIANT TypeOK\nINVARIANT NoNewOrderAfterHalt\n"),
 ("mutants", "M3_ReduceOnlyFlattenAlwaysApprovable",
  "risk::check vetoes everything while halted, including the reduce-only flatten",
  [("  IF ro /\\ Abs(x + side * qty) < Abs(x) /\\ Sign(x + side * qty) * Sign(x) >= 0\n  THEN TRUE\n",
    "  IF FALSE\n  THEN TRUE\n")],
  "INVARIANT TypeOK\nINVARIANT ReduceOnlyFlattenAlwaysApprovable\n"),
 ("mutants", "M4_FilledQtyNeverExceedsOrderQty",
  "Fill payload validation removed: an unevidenced overfilling report is applied",
  [("  /\\ HaltG\n  /\\ last' = <<o[i].st, \"Fill\", \"PayloadError\">>\n  /\\ UNCHANGED <<o, v, riskHalted, kills, killed, lpos, pos, foreign, fresh>>\n",
    "  /\\ o' = [o EXCEPT ![i].cum = o[i].qty + 1, ![i].st = \"Filled\"]\n  /\\ lpos' = lpos + o[i].side * (o[i].qty + 1 - o[i].cum)\n  /\\ last' = <<o[i].st, \"Fill\", \"Filled\">>\n  /\\ UNCHANGED <<v, ap, gh, halted, flag, riskHalted, kills, killed, pos, foreign, fresh>>\n")],
  "INVARIANT TypeOK\nINVARIANT FilledQtyNeverExceedsOrderQty\n"),
 ("mutants", "M5_CancelRemainingConsistent",
  "Cancelled{remaining} emitted before evidenced racing fills drain",
  [("  /\\ o[i].st \\in {\"Canceled\", \"Expired\"} /\\ o[i].rem = -1\n  /\\ o[i].cum = V(i).vcum\n",
    "  /\\ o[i].st \\in {\"Canceled\", \"Expired\"} /\\ o[i].rem = -1\n")],
  "INVARIANT TypeOK\nINVARIANT CancelRemainingConsistent\n"),
 ("mutants", "M6_HaltOnlyClearedByOperatorReset",
  "a clean reconciliation re-arms the book (clears the latch) without OperatorReset",
  [("          /\\ UNCHANGED <<halted, flag, ap, gh>>   \\* a match keeps the latch set\n",
    "          /\\ halted' = FALSE /\\ flag' = FALSE /\\ UNCHANGED <<ap, gh>>\n")],
  "INVARIANT TypeOK\nPROPERTY HaltOnlyClearedByOperatorReset\n"),
 ("mutants", "M7_NoTwoLiveVenueOrdersPerIntent",
  "F3 regression: NotFound treated as definitive before recvWindow + margin elapsed",
  [("  /\\ o[i].el > Bound\n  /\\ IF o[i].att + 1 < MaxAttempts\n",
    "  /\\ IF o[i].att + 1 < MaxAttempts\n")],
  "INVARIANT TypeOK\nINVARIANT NoTwoLiveVenueOrdersPerIntent\n"),
 ("mutants", "M8_FillsAcrossAttemptsWithinApproved",
  "an Unknown order found EXPIRED (possibly partially filled) is retried for its full qty under a fresh id",
  [(QEXP_OLD,
    '''QExpired(i) ==   \\* MUTANT: expired treated as absent, full-qty retry
  /\\ o[i].st = "Unknown" /\\ V(i).vst = "expired" /\\ o[i].att + 1 < MaxAttempts
  /\\ o' = [o EXCEPT ![i].st = "PendingNew", ![i].att = @ + 1, ![i].sent = FALSE, ![i].cum = 0,
                    ![i].resp = "none", ![i].cxl = FALSE, ![i].cresp = "none",
                    ![i].el = 0, ![i].acked = FALSE]
  /\\ lpos' = lpos
  /\\ last' = <<"Unknown", "QueryExpired", "PendingNew">>
''')],
  "INVARIANT TypeOK\nINVARIANT FillsAcrossAttemptsWithinApproved\n"),
 ("mutants", "M9_ApprovalsNotUsedAcrossHalt",
  "F8 regression: halts do not bump halt_epoch (approvals survive halt + OperatorReset)",
  [("Bump == ap' = [j \\in Intents |-> FALSE]", "Bump == UNCHANGED ap")],
  "INVARIANT TypeOK\nINVARIANT ApprovalsNotUsedAcrossHalt\n"),
 ("mutants", "M10_NoSubmitOfPreKillOrders",
  "F9 regression: kill does not bump halt_epoch, so a pre-kill reduce-only order can still be dispatched",
  [("  /\\ killed' = TRUE /\\ halted' = TRUE /\\ kills' = kills + 1\n  /\\ Bump\n",
    "  /\\ killed' = TRUE /\\ halted' = TRUE /\\ kills' = kills + 1\n  /\\ UNCHANGED ap\n")],
  "INVARIANT TypeOK\nINVARIANT NoSubmitOfPreKillOrders\n"),
 ("mutants", "M11_NoGhostAcked",
  "F5 regression: CancelDenied with no processed Ack maps to Acked (v3 behavior)",
  [("                    ELSE IF o[i].acked THEN \"Acked\"\n                    ELSE \"Unknown\" IN\n",
    "                    ELSE \"Acked\" IN\n")],
  "INVARIANT TypeOK\nINVARIANT NoGhostAcked\n"),
 ("mutants", "R1_F1_NoQueryExpired",
  "F1 regression: remove the QueryExpired resolution (v3 had no such event)",
  [("QExpired(i) ==   \\* F1: EXPIRED / EXPIRED_IN_MATCH resolves Unknown\n",
    "QExpired(i) ==   \\* REGRESSION: v3 has no QueryExpired\n  /\\ FALSE\n")],
  "INVARIANT TypeOK\nPROPERTY UnknownResolvesWhenVenueDone\n"),
 ("mutants", "R2_F2_SameIdRetryAfterPurge",
  "F2 regression: after a definitive NotFound the retry reuses the same ClientOrderId (v3 behavior); "
  "the venue purged the old CANCELED/EXPIRED unfilled order, so the same id is sent twice",
  [("![i].att = @ + 1, ![i].sent = FALSE,\n                     ![i].resp", "![i].att = @, ![i].sent = FALSE,\n                     ![i].resp")],
  "INVARIANT TypeOK\nINVARIANT NoDuplicateSubmitPerClientOrderId\n"),
 ("findings", "W1_F2_PurgeScenarioReachable",
  "reachability witness (expected to be violated): the F2 scenario (order landed, then purged, then a "
  "definitive NotFound and a retry) is reachable in v3.1 and is retried under a fresh id",
  [("=============================================================================\n",
    "PurgedOrderNeverRetried == ~(\\E i \\in Intents : o[i].att > 0 /\\ v[i][0].vsubs > 0)\n\n"
    "=============================================================================\n")],
  "INVARIANT TypeOK\nINVARIANT PurgedOrderNeverRetried\n"),
]
only = set(sys.argv[2:])
for group, name, why, reps, checks in VARIANTS:
    if only and name not in only: continue
    src = BASE
    for old, new in reps:
        assert src.count(old) == 1, (name, old[:60], src.count(old))
        src = src.replace(old, new)
    d = os.path.join(group, name)
    shutil.rmtree(d, ignore_errors=True); os.makedirs(d)
    open(os.path.join(d, "OrderLifecycle.tla"), "w").write(src)
    open(os.path.join(d, "OrderLifecycle.cfg"), "w").write(CONSTS + checks)
    diff = "".join(difflib.unified_diff(BASE.splitlines(True), src.splitlines(True),
                                        "OrderLifecycle.tla", f"{d}/OrderLifecycle.tla"))
    open(os.path.join(d, "mutation.diff"), "w").write(f"# {why}\n" + diff)
    meta = f"/tmp/tlc-{name}"
    shutil.rmtree(meta, ignore_errors=True)
    cmd = ["java", "-XX:+UseParallelGC", "-Xmx512m", "-cp", JAR, "tlc2.TLC", "-workers", "1",
           "-metadir", meta, "-config", "OrderLifecycle.cfg", "OrderLifecycle.tla"]
    r = subprocess.run(cmd, cwd=d, capture_output=True, text=True)
    log = r.stdout + r.stderr
    open(os.path.join(d, "tlc.log"), "w").write("$ " + " ".join(cmd) + "\n" + log + f"\nexit_code={r.returncode}\n")
    verdict = [l for l in log.splitlines() if l.startswith("Error:") and ("violated" in l or "Deadlock" in l or "Temporal" in l)]
    stats = [l for l in log.splitlines() if "distinct states found" in l and "Progress" not in l]
    depth = [l for l in log.splitlines() if l.startswith("The depth")]
    print(f"{name}: exit={r.returncode} {verdict[:1]} {stats[-1:] } {depth}", flush=True)
