#!/usr/bin/env python3
"""Check the bounded evidence-lifetime model and its named fallback fault."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

JAR_SHA256 = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"
MODEL = "EvidenceLifetime.tla"
CASES = (
    ("same-trial", "EvidenceLifetime.cfg", None),
    ("distinct-trials", "EvidenceLifetime-distinct-trials.cfg", None),
    ("bad-fallback", "EvidenceLifetime-bad-fallback.cfg", "KnownEntryCannotBecomePreSpawn"),
)
TIMEOUT_SECONDS = 120


def digest(data):
    return hashlib.sha256(data).hexdigest()


def write_new_json(path, value):
    with path.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, indent=2)
        stream.write("\n")


def state_counts(output):
    match = re.search(
        r"([\d,]+) states generated, ([\d,]+) distinct states found, "
        r"([\d,]+) states left on queue", output,
    )
    if match is None:
        return None
    return dict(zip(
        ("generated", "distinct", "queued"),
        (int(part.replace(",", "")) for part in match.groups()),
    ))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jar", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True, help="New private evidence directory")
    args = parser.parse_args()
    jar = args.jar.resolve(strict=True)
    if digest(jar.read_bytes()) != JAR_SHA256:
        parser.error("JAR differs from the repository-pinned TLA+ 1.7.4 artifact")
    for name in ("JAVA_TOOL_OPTIONS", "_JAVA_OPTIONS", "JDK_JAVA_OPTIONS"):
        if os.environ.get(name):
            parser.error("JVM override is outside this bounded runner: " + name)
    source = Path(__file__).resolve().parent
    names = (MODEL, *(row[1] for row in CASES))
    inputs = {name: (source / name).read_bytes() for name in names}
    identities = {name: digest(data) for name, data in inputs.items()}
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    results = []
    for name, config, invariant in CASES:
        case = out / name
        case.mkdir()
        for filename in (MODEL, config):
            (case / filename).write_bytes(inputs[filename])
        states = case / "states"
        command = [
            "java", "-XX:+UseParallelGC", "-Xmx512m", "-cp", str(jar),
            "tlc2.TLC", "-workers", "1", "-seed", "0", "-fp", "0",
            "-metadir", str(states), "-config", config, "EvidenceLifetime",
        ]
        row = {
            "case": name, "command": command, "config": config,
            "expected_invariant_failure": invariant, "spawn_attempted": False,
            "pid": None, "exit": None, "timeout": False, "operation_error": None,
        }
        write_new_json(case / "command.json", row)
        process = None
        try:
            with (case / "tlc.log").open("x", encoding="utf-8") as log:
                row["spawn_attempted"] = True
                process = subprocess.Popen(
                    command, cwd=case, stdout=log, stderr=subprocess.STDOUT,
                )
                row["pid"] = process.pid
                try:
                    row["exit"] = process.wait(timeout=TIMEOUT_SECONDS)
                except subprocess.TimeoutExpired:
                    row["timeout"] = True
                    process.kill()
                    row["exit"] = process.wait(timeout=10)
        except (OSError, subprocess.SubprocessError) as error:
            row["operation_error"] = type(error).__name__ + ": " + str(error)
            if process is not None:
                row["exit"] = process.poll()
                if row["exit"] is None:
                    try:
                        process.kill()
                        row["exit"] = process.wait(timeout=10)
                    except (OSError, subprocess.SubprocessError) as cleanup_error:
                        row["process_cleanup_error"] = str(cleanup_error)
        try:
            output = (case / "tlc.log").read_text(encoding="utf-8", errors="replace")
            row["states"] = state_counts(output)
            unchanged = all((source / path).read_bytes() == data for path, data in inputs.items())
            row["source_unchanged"] = unchanged
            row["jar_unchanged"] = digest(jar.read_bytes()) == JAR_SHA256
            healthy_io = not row["timeout"] and row["operation_error"] is None
            if invariant is None:
                accepted = (
                    healthy_io and row["exit"] == 0
                    and "Model checking completed. No error has been found." in output
                    and row["states"] is not None and row["states"]["queued"] == 0
                )
            else:
                accepted = (
                    healthy_io and row["exit"] == 12
                    and ("Error: Invariant " + invariant + " is violated.") in output
                    and "unexpected exception" not in output
                )
            row["expectation_met"] = accepted and unchanged and row["jar_unchanged"]
        except OSError as error:
            row["evidence_error"] = type(error).__name__ + ": " + str(error)
            row["expectation_met"] = False
        write_new_json(case / "result.json", row)
        results.append(row)
        print(name + ": " + ("expected result" if row["expectation_met"] else "UNEXPECTED RESULT"), flush=True)
        if row["expectation_met"] and states.exists():
            shutil.rmtree(states)
        if not row["expectation_met"]:
            break
    report = {
        "schema": "parangsae-evidence-lifetime-ordinary-tlc-v1",
        "scope": "Two units/Unit-to-Trial image, ordinary TLC safety; no SDK owner or runtime/durability proof",
        "source": str(source), "input_sha256": identities, "jar_sha256": JAR_SHA256,
        "heap_limit_mib": 512, "workers": 1, "timeout_seconds_per_case": TIMEOUT_SECONDS,
        "results": results,
        "all_expected": len(results) == len(CASES) and all(r["expectation_met"] for r in results),
    }
    write_new_json(out / "result.json", report)
    return 0 if report["all_expected"] else 1


if __name__ == "__main__":
    sys.exit(main())
