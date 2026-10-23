"""Run with python3 -m unittest discover -s specs/tla/execution -v."""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import unittest


HERE = Path(__file__).resolve().parent
GOOD = (HERE / "tlc.log").read_text()
CELLS = re.findall(r'<<\s*"CELL",\s*"\w+",\s*"\w+",\s*\{[^}]*\}\s*>>',
                   re.sub(r"\s*\n\s*", " ", GOOD))
MATRIX = "\n".join(CELLS) + "\n"


class GenerateTransitionsTest(unittest.TestCase):
    def generate(self, root, log):
        (root / "input.log").write_text(log)
        return subprocess.run(
            [sys.executable, str(HERE / "gen_transitions.py"), "input.log",
             "transitions.json", "diff.md"], cwd=root, capture_output=True, text=True)

    def test_committed_good_log(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            result = self.generate(root, GOOD)
            self.assertEqual(result.returncode, 0, result.stderr)
            doc = json.loads((root / "transitions.json").read_text())
            self.assertEqual(doc["checks"], json.loads((HERE / "transitions.json").read_text())["checks"])
            self.assertTrue((root / "diff.md").is_file())

    def test_failed_logs_preserve_outputs(self):
        invariant = (HERE / "mutants/M1_NoDuplicateSubmitPerClientOrderId/tlc.log").read_text()
        property_failure = (HERE / "mutants/M6_HaltOnlyClearedByOperatorReset/tlc.log").read_text()
        cases = {
            "truncated": MATRIX,
            "oom": MATRIX + "Error: Java heap space\nexit_code=1\n",
            "invariant": invariant,
            "property": property_failure,
            "missing_cell": GOOD.replace(CELLS[0], "", 1),
            "duplicate_cell": GOOD + CELLS[0],
            "wrong_cell": GOOD.replace(CELLS[0], CELLS[0].replace('{"Acked"}', '{}'), 1),
        }
        for name, log in cases.items():
            for existing in (False, True):
                with self.subTest(log=name, existing=existing), tempfile.TemporaryDirectory() as tmp:
                    root = Path(tmp)
                    outputs = [root / "transitions.json", root / "diff.md"]
                    if existing:
                        for path in outputs:
                            path.write_text("trusted evidence")
                    result = self.generate(root, log)
                    self.assertNotEqual(result.returncode, 0, result.stdout)
                    for path in outputs:
                        if existing:
                            self.assertEqual(path.read_text(), "trusted evidence")
                        else:
                            self.assertFalse(path.exists())

    def test_runner_preserves_evidence_on_failure(self):
        for status, log in ((137, MATRIX), (1, MATRIX + "Error: Java heap space\n"),
                            (12, GOOD), (0, MATRIX), (0, GOOD)):
            with self.subTest(status=status), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                for name in ("run_tlc.sh", "gen_transitions.py"):
                    shutil.copy(HERE / name, root / name)
                outputs = [root / name for name in ("tlc.log", "transitions.json",
                           "transitions-v3-to-v3.1.md", "lifecycle.dot", "lifecycle.svg")]
                for path in outputs:
                    path.write_text("trusted evidence")
                (root / "input.log").write_text(log)
                for name, script in {
                    "sha256sum": "cat >/dev/null\n",
                    "java": f"cat input.log\nexit {status}\n",
                }.items():
                    path = root / name
                    path.write_text("#!/bin/sh\n" + script)
                    path.chmod(0o755)
                result = subprocess.run(["sh", "run_tlc.sh"], cwd=root,
                                        env={**os.environ, "PATH": f"{root}:{os.environ['PATH']}"},
                                        capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0, result.stdout)
                for path in outputs:
                    self.assertEqual(path.read_text(), "trusted evidence", path.name)


if __name__ == "__main__":
    unittest.main()
