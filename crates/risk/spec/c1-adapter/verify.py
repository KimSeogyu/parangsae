#!/usr/bin/env python3
"""Check the finite adapter product and two deliberate regressions."""

import argparse
import hashlib
import json
from pathlib import Path
import os
import re
import shutil
import subprocess
import sys

JAR_SHA256 = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"
MODEL = "C1AdapterObligations.tla"
CASES = (
    ("baseline", "C1AdapterObligations.cfg", None),
    ("old-wait", "C1AdapterObligationsBadWait.cfg", "RiskTickRequestsClose"),
    ("strict-cut", "C1AdapterObligationsBadStrict.cfg", "LawfulEqualityCompletes"),
)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jar", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    jar = args.jar.resolve(strict=True)
    if digest(jar) != JAR_SHA256:
        parser.error("JAR differs from the repository-pinned TLA+ 1.7.4 artifact")
    for name in ("JAVA_TOOL_OPTIONS", "_JAVA_OPTIONS", "JDK_JAVA_OPTIONS"):
        if os.environ.get(name):
            parser.error("JVM override is outside this bounded runner: " + name)
    source = Path(__file__).resolve().parent
    inputs = [MODEL, *(row[1] for row in CASES)]
    identities = {name: digest(source / name) for name in inputs}
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    for name in inputs:
        shutil.copyfile(source / name, out / name)
    results = []
    for name, config, invariant in CASES:
        states = out / ("states-" + name)
        command = [
            "java", "-XX:+UseParallelGC", "-Xmx512m", "-cp", str(jar),
            "tlc2.TLC", "-workers", "1", "-seed", "1",
            "-metadir", str(states), "-config", config, MODEL,
        ]
        timeout = False
        try:
            result = subprocess.run(
                command, cwd=out, text=True, stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT, timeout=120, check=False,
            )
            output = result.stdout
            code = result.returncode
        except subprocess.TimeoutExpired as error:
            timeout = True
            output = error.stdout or b""
            if isinstance(output, bytes):
                output = output.decode("utf-8", errors="replace")
            code = None
        (out / (name + ".log")).write_text(output)
        counts = re.search(
            r"([\d,]+) states generated, ([\d,]+) distinct states found, "
            r"([\d,]+) states left on queue", output,
        )
        counts = None if counts is None else {
            key: int(value.replace(",", ""))
            for key, value in zip(("generated", "distinct", "queued"), counts.groups())
        }
        if invariant is None:
            passed = (
                not timeout and code == 0
                and "Model checking completed. No error has been found." in output
                and counts is not None and counts["queued"] == 0
            )
        else:
            passed = (
                not timeout and code is not None and code != 0
                and ("Invariant " + invariant + " is violated") in output
            )
        unchanged = all(digest(source / path) == value for path, value in identities.items())
        passed = passed and unchanged
        results.append({
            "name": name, "config": config, "command": command,
            "expected_invariant_failure": invariant, "exit_code": code,
            "timeout": timeout, "states": counts, "expectation_met": passed,
            "source_unchanged": unchanged, "log": str(out / (name + ".log")),
        })
        print(name + ": " + ("expected result" if passed else "UNEXPECTED RESULT"), flush=True)
        if states.exists():
            shutil.rmtree(states)
        if not passed:
            print(output[-6000:], file=sys.stderr)
            break
    report = {
        "scope": "Finite adapter obligation model; no runtime, money or source-classification proof",
        "model_source": str(source), "input_sha256": identities,
        "jar_sha256": JAR_SHA256, "heap_limit_mib": 512,
        "workers": 1, "timeout_seconds_per_case": 120,
        "results": results,
        "all_expected": len(results) == len(CASES)
        and all(row["expectation_met"] for row in results),
    }
    (out / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    return 0 if report["all_expected"] else 1


if __name__ == "__main__":
    sys.exit(main())
