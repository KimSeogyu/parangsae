#!/usr/bin/env python3
"""Check the finite CarryCapital design and three intentional broken controls."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys

JAR_SHA256 = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"
CHECKS = (
    ("baseline", None, None),
    ("short-principal", "BugShortPrincipal", "NoShortPrincipalCash"),
    ("cancel-release", "BugReleaseOnCancel", "NeverReleaseUnreconciledReservation"),
    ("future-spot", "BugHedgeBeforeVisible", "HedgeOnlyAfterVisibleFirstLegReceipt"),
    ("full-cycle-reachable", None, "FullCycleUnreachable"),
)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jar", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True, help="New private evidence directory")
    args = parser.parse_args()
    jar = args.jar.resolve(strict=True)
    if hashlib.sha256(jar.read_bytes()).hexdigest() != JAR_SHA256:
        parser.error("jar is not the repository-pinned TLA+ 1.7.4 artifact")
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    source = Path(__file__).resolve().parent
    base_config = (source / "CarryCapital.cfg").read_text()
    shutil.copyfile(source / "CarryCapital.tla", out / "CarryCapital.tla")
    results = []
    for name, flag, invariant in CHECKS:
        config = base_config
        if flag is not None:
            old = flag + " = FALSE"
            if config.count(old) != 1:
                raise RuntimeError("missing or ambiguous broken-control flag: " + flag)
            config = config.replace(old, flag + " = TRUE")
        elif invariant is not None:
            config += "  " + invariant + "\n"
        cfg = out / (name + ".cfg")
        cfg.write_text(config)
        states = out / ("states-" + name)
        command = [
            "java", "-XX:+UseParallelGC", "-Xmx256m", "-cp", str(jar),
            "tlc2.TLC", "-workers", "1", "-seed", "1",
            "-metadir", str(states), "-config", cfg.name, "CarryCapital.tla",
        ]
        result = subprocess.run(
            command, cwd=out, text=True, stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT, timeout=300, check=False,
        )
        (out / (name + ".log")).write_text(result.stdout)
        if invariant is None:
            passed = (
                result.returncode == 0
                and "Model checking completed. No error has been found." in result.stdout
            )
        else:
            passed = (
                result.returncode != 0
                and ("Invariant " + invariant + " is violated") in result.stdout
            )
        count_match = re.search(
            r"([\d,]+) states generated, ([\d,]+) distinct states found, "
            r"([\d,]+) states left on queue", result.stdout,
        )
        counts = None if count_match is None else {
            key: int(value.replace(",", ""))
            for key, value in zip(("generated", "distinct", "queued"), count_match.groups())
        }
        results.append({
            "name": name, "expected_invariant_failure": invariant,
            "exit_code": result.returncode, "expectation_met": passed,
            "states": counts, "log": str(out / (name + ".log")),
        })
        print(name + ": " + ("expected result" if passed else "UNEXPECTED RESULT"), flush=True)
        if states.exists():
            shutil.rmtree(states)
        if not passed:
            print(result.stdout[-6000:], file=sys.stderr)
            break
    report = {
        "scope": "Finite design model only; not a proof of Rust/account/journal correctness",
        "model": str(source / "CarryCapital.tla"),
        "model_sha256": hashlib.sha256((source / "CarryCapital.tla").read_bytes()).hexdigest(),
        "config_sha256": hashlib.sha256((source / "CarryCapital.cfg").read_bytes()).hexdigest(),
        "jar_sha256": JAR_SHA256, "worker_count": 1, "heap_limit_mib": 256,
        "checks": results, "all_expected": len(results) == len(CHECKS)
        and all(row["expectation_met"] for row in results),
    }
    (out / "verification.json").write_text(json.dumps(report, indent=2) + "\n")
    return 0 if report["all_expected"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
