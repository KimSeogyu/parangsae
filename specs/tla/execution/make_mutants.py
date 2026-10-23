#!/usr/bin/env python3
"""Build mutants/ and findings/ variants of OrderLifecycle.tla by exact text
replacement (each replacement must match exactly once), then run TLC on each with
the CI command shape. Usage: python3 make_mutants.py /path/to/tla2tools.jar"""
import os, subprocess, sys, difflib, shutil
JAR = sys.argv[1]
BASE = open("OrderLifecycle.tla").read()
CONSTS = """SPECIFICATION Spec
CONSTANT Intents = {i1, i2}
CONSTANT MaxQty = 2
CONSTANT MaxAge = 0
CONSTANT MaxEpoch = 1
CONSTANT InitPos = {0, 1}
CONSTANT InitForeign = {FALSE, TRUE}

"""
VARIANTS = [
 ("mutants", "M1_NoDuplicateSubmitPerClientOrderId",
  "QueryNotFound treated as authoritative while the original NEW may still be in flight",
  "  /\\ o[i].st = \"Unknown\" /\\ o[i].vst = \"none\" /\\ ~o[i].infl /\\ o[i].cum = 0\n",
  "  /\\ o[i].st = \"Unknown\" /\\ o[i].vst = \"none\" /\\ o[i].cum = 0\n",
  "INVARIANT TypeOK\nINVARIANT NoDuplicateSubmitPerClientOrderId\n"),
 ("mutants", "M2_NoNewOrderAfterHalt",
  "dispatch gate ignores the halt latch (checks only epoch and age)",
  "  /\\ \\/ (~halted /\\ ~riskHalted)\n     \\/ (o[i].ro /\\ StrictReduce(i) /\\ ExposureKnown)\n",
  "  /\\ \\/ TRUE\n     \\/ (o[i].ro /\\ StrictReduce(i) /\\ ExposureKnown)\n",
  "INVARIANT TypeOK\nINVARIANT NoNewOrderAfterHalt\n"),
 ("mutants", "M3_ReduceOnlyFlattenAlwaysApprovable",
  "risk::check vetoes everything while halted, including the reduce-only flatten",
  "  IF ro /\\ Abs(x + side * qty) < Abs(x) /\\ Sign(x + side * qty) * Sign(x) >= 0\n  THEN TRUE\n",
  "  IF FALSE\n  THEN TRUE\n",
  "INVARIANT TypeOK\nINVARIANT ReduceOnlyFlattenAlwaysApprovable\n"),
 ("mutants", "M4_FilledQtyNeverExceedsOrderQty",
  "Fill payload validation removed: an unevidenced overfilling report is applied",
  "  /\\ Halt\n  /\\ last' = <<o[i].st, \"Fill\", \"PayloadError\">>\n  /\\ UNCHANGED <<o, riskHalted, epoch, killed, lpos, pos, foreign, fresh>>\n",
  "  /\\ o' = [o EXCEPT ![i].cum = o[i].qty + 1, ![i].st = \"Filled\"]\n  /\\ lpos' = lpos + o[i].side * (o[i].qty + 1 - o[i].cum)\n  /\\ last' = <<o[i].st, \"Fill\", \"Filled\">>\n  /\\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, pos, foreign, fresh>>\n",
  "INVARIANT TypeOK\nINVARIANT FilledQtyNeverExceedsOrderQty\n"),
 ("mutants", "M5_CancelRemainingConsistent",
  "Cancelled{remaining} emitted before evidenced racing fills drain",
  "  /\\ o[i].st \\in {\"Canceled\", \"Expired\"} /\\ o[i].rem = -1\n  /\\ o[i].cum = o[i].vcum\n",
  "  /\\ o[i].st \\in {\"Canceled\", \"Expired\"} /\\ o[i].rem = -1\n",
  "INVARIANT TypeOK\nINVARIANT CancelRemainingConsistent\n"),
 ("mutants", "M6_HaltOnlyClearedByOperatorReset",
  "a clean reconciliation re-arms the book (clears the latch) without OperatorReset",
  "          /\\ UNCHANGED <<halted, flag>>          \\* a match keeps the latch set\n",
  "          /\\ halted' = FALSE /\\ flag' = FALSE\n",
  "INVARIANT TypeOK\nPROPERTY HaltOnlyClearedByOperatorReset\n"),
 ("findings", "F2_ArchivedOrderNotFound_DuplicateSubmit",
  "venue doc behavior: CANCELED/EXPIRED orders with no fills stop being found after 3 days (-2013); "
  "the venue forgets them, so v3's QueryNotFound -> same-ID retry resubmits a CID that already existed",
  "VCancel(i) ==\n",
  "VForget(i) ==   \\* Query Order: CANCELED/EXPIRED, no fill, created + 3 days < now -> not found\n"
  "  /\\ o[i].vst \\in {\"canceled\", \"expired\"} /\\ o[i].vcum = 0\n"
  "  /\\ o' = [o EXCEPT ![i].vst = \"none\"]\n"
  "  /\\ fresh' = FALSE\n"
  "  /\\ UNCHANGED <<halted, flag, riskHalted, epoch, killed, lpos, pos, foreign, last>>\n\n"
  "VCancel(i) ==\n",
  "INVARIANT TypeOK\nINVARIANT NoDuplicateSubmitPerClientOrderId\n"),
 ("findings", "F1_UnknownExpiredNeverResolves",
  "an Unknown order the venue EXPIRED (IOC/GTX/market expiry, EXPIRED_IN_MATCH) has no resolving v3 event",
  "KillEventuallyFlatOrFlagged ==\n",
  "UnknownExpiredResolves ==\n  \\A i \\in Intents : (o[i].st = \"Unknown\" /\\ o[i].vst = \"expired\") ~> (o[i].st # \"Unknown\")\n\n"
  "KillEventuallyFlatOrFlagged ==\n",
  "INVARIANT TypeOK\nPROPERTY UnknownExpiredResolves\n"),
]
for group, name, why, old, new, checks in VARIANTS:
    assert BASE.count(old) == 1, (name, BASE.count(old))
    src = BASE.replace(old, new)
    if name.startswith("F2"):   # also add VForget to Next
        o2 = "       \\/ VLand(i) \\/ VDrop(i) \\/ VFill(i) \\/ VExpire(i) \\/ VCancel(i)\n"
        assert src.count(o2) == 1
        src = src.replace(o2, o2.replace("VCancel(i)\n", "VCancel(i) \\/ VForget(i)\n"))
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
    print(f"{name}: exit={r.returncode} {verdict[:2]}")
