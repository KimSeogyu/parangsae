"""Synthetic-only observable checks. No market data or real t constants."""

from copy import deepcopy
from datetime import date, timedelta
from fractions import Fraction
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "astra_inference.py"
SPEC = importlib.util.spec_from_file_location("astra_inference", SCRIPT)
inference = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = inference
SPEC.loader.exec_module(inference)

C0 = 10_000_000_000_000
PAIRS = (("shock_primary", "shock_direction_control"),
         ("range_primary", "range_direction_control"),
         ("premium_primary", "premium_price_control"))
SOURCE = "SYNTHETIC_NO_RUN:independent-closed-form-k1"


def fixture(days=1120):
    start = date(2020, 12, 18)
    primary = []
    cash = []
    equity = C0
    for index in range(days):
        equity += 1_000_000_000 + (100_000 if index // 56 % 2 == 0 else -100_000)
        day = (start + timedelta(days=index)).isoformat()
        primary.append({"day": day, "equity_ticks": equity})
        cash.append({"day": day, "equity_ticks": C0})
    roles = []
    for left, right in PAIRS:
        roles.extend((
            {"role": left, "status": "COMPLETE", "exposed": True, "daily": deepcopy(primary)},
            {"role": right, "status": "COMPLETE", "exposed": False, "daily": deepcopy(cash)},
        ))
    inputs = {"schema": "astra-cash-evidence-v1", "scope": "SYNTHETIC_NO_RUN",
              "start_day": start.isoformat(), "days": days, "c0_ticks": C0,
              "invocation_status": "COMPLETE", "roles": roles}
    brackets = [{"block_days": length, "df": days // length - 1,
                 "lower": {"num": "1", "den": "1"},
                 "upper": {"num": "1", "den": "1"}}
                for length in (56, 28)] if days // 56 >= 20 else []
    constants = {"schema": "astra-critical-enclosures-v1", "scope": "SYNTHETIC_NO_RUN",
                 "start_day": start.isoformat(), "days": days, "source_id": SOURCE,
                 "certificate_sha256": hashlib.sha256(b"SYNTHETIC_NOT_STUDENT_T").hexdigest(),
                 "probability": {"num": 239, "den": 240}, "brackets": brackets}
    return inputs, constants


def calculate(inputs, constants):
    cash = inference.parse_cash(inputs)
    critical = inference.parse_critical(constants, cash, SOURCE)
    return inference.analyze(cash, critical)


def as_fraction(value):
    return Fraction(int(value["num"]), int(value["den"]))


class ArithmeticTests(unittest.TestCase):
    def test_independent_closed_form_oracle_and_full_calendar(self):
        # Statistical Auditor derived these without reading/importing the reducer.
        report = calculate(*fixture())
        expected = {"56": Fraction(5329, 7_600_000_000_000_000),
                    "28": Fraction(5329, 15_600_000_000_000_000)}
        for family in ("shock", "range", "premium"):
            row = report["families"][family]
            self.assertEqual(as_fraction(row["annual_simple_A"]), Fraction(73, 2000))
            self.assertEqual(as_fraction(row["economic_H"]["point"]), Fraction(33, 2000))
            self.assertEqual(as_fraction(row["direction_D"]["point"]), Fraction(73, 2000))
            self.assertEqual(row["numeric_result"], "NUMERIC_LOCAL_SUPPORT")
            for endpoint in ("economic_H", "direction_D"):
                for length, variance in expected.items():
                    item = row[endpoint]["by_block_days"][length]
                    self.assertEqual(as_fraction(item["se_squared"]), variance)
                    self.assertEqual((item["status"], item["precision"]), ("SUPPORT", "PASS"))
                    edges = item["boundaries"]
                    self.assertEqual((edges[0], edges[-1]), (0, 1120))
                    self.assertEqual(sum(b - a for a, b in zip(edges, edges[1:])), 1120)
        self.assertFalse(report["advance_authority"])
        self.assertFalse(report["cdf_certificate_verified_here"])
        self.assertFalse(report["producer_custody_verified_here"])

    def test_balanced_partition_keeps_nondivisible_tail(self):
        report = calculate(*fixture(1133))
        for length in ("56", "28"):
            edges = report["families"]["shock"]["economic_H"]["by_block_days"][length]["boundaries"]
            sizes = [right - left for left, right in zip(edges, edges[1:])]
            self.assertEqual((edges[0], edges[-1], sum(sizes)), (0, 1133, 1133))
            self.assertLessEqual(max(sizes) - min(sizes), 1)

    def test_short_calendar_does_not_construct_interval(self):
        report = calculate(*fixture(1119))
        item = report["families"]["shock"]["economic_H"]["by_block_days"]["56"]
        self.assertEqual(item["status"], "INELIGIBLE")
        self.assertNotIn("se_squared", item)
        self.assertNotIn("boundaries", item)

    def test_zero_se_and_cash_only_are_undefined_not_fail(self):
        inputs, constants = fixture()
        for role in inputs["roles"]:
            role["exposed"] = False
            for row in role["daily"]:
                row["equity_ticks"] = C0
        report = calculate(inputs, constants)
        self.assertEqual(report["families"]["shock"]["economic_H"]["by_block_days"]["56"]["status"],
                         "UNDEFINED_NO_EXPOSURE")
        for role in inputs["roles"][::2]:
            role["exposed"] = True
        report = calculate(inputs, constants)
        self.assertEqual(report["families"]["shock"]["direction_D"]["by_block_days"]["56"]["status"],
                         "UNDEFINED_ZERO_SE")

    def test_one_cash_tick_loss_keeps_exact_sign(self):
        inputs, constants = fixture()
        for role in inputs["roles"][::2]:
            for row in role["daily"]:
                row["equity_ticks"] = C0
            role["daily"][-1]["equity_ticks"] -= 1
        row = calculate(inputs, constants)["families"]["shock"]
        self.assertEqual(as_fraction(row["annual_simple_A"]), Fraction(-73, 2_240_000_000_000_000))
        self.assertEqual(row["numeric_result"], "OBSERVED_PRIMARY_CASH_LOSS")

    def test_endpoint_equalities_and_numeric_ambiguity(self):
        checks = ((Fraction(1, 100), Fraction(1, 100), "INSUFFICIENT", "PASS"),
                  (Fraction(-1, 100), Fraction(1, 100), "FAILURE", "PASS"),
                  (Fraction(1, 25), Fraction(1, 50), "SUPPORT", "PASS"),
                  (Fraction(-3, 100), Fraction(3, 100), "FAILURE", "FAIL"))
        for point, width, status, precision in checks:
            self.assertEqual(inference.classify_squared(point, width**2, width**2), (status, precision))
        self.assertEqual(inference.classify_squared(Fraction(1, 50), Fraction(1, 100)**2,
                                                   Fraction(3, 100)**2),
                         ("NUMERIC_UNDEFINED", "NUMERIC_UNDEFINED"))

    def test_outward_sqrt_encloses_irrational_and_exact_ties(self):
        low, high = inference.sqrt_enclosure(Fraction(2))
        self.assertLessEqual(low * low, 2)
        self.assertGreaterEqual(high * high, 2)
        self.assertEqual(inference.sqrt_enclosure(Fraction(1, 2500)),
                         (Fraction(1, 50), Fraction(1, 50)))

    def test_wide_consistent_failure_precedes_precision(self):
        cells = {str(length): {"status": "FAILURE", "precision": "FAIL"} for length in (56, 28)}
        evidence = {"cash_sum_ticks": "1", "by_block_days": cells}
        role = inference.Role("COMPLETE", True, ())
        self.assertEqual(inference.family_result(role, role, evidence, evidence), "ECONOMIC_ENDPOINT_FAILURE")
        mixed = deepcopy(evidence)
        mixed["by_block_days"]["28"]["status"] = "SUPPORT"
        other = {"cash_sum_ticks": "1", "by_block_days":
                 {str(length): {"status": "INSUFFICIENT", "precision": "PASS"} for length in (56, 28)}}
        self.assertEqual(inference.family_result(role, role, mixed, other), "INCONCLUSIVE_DEPENDENCE")

    def test_incomplete_control_and_descriptive_risk_have_different_scope(self):
        inputs, constants = fixture()
        long_role = deepcopy(inputs["roles"][0])
        long_role["role"], long_role["status"] = "shock_long_reference", "RISK_STOP"
        long_role["daily"] = long_role["daily"][:1]
        inputs["roles"].append(long_role)
        self.assertEqual(calculate(inputs, constants)["families"]["shock"]["numeric_result"],
                         "NUMERIC_LOCAL_SUPPORT")
        inputs["roles"][1]["status"] = "RISK_STOP"
        row = calculate(inputs, constants)["families"]["shock"]
        self.assertIsNone(row["direction_D"])
        self.assertIsNotNone(row["annual_simple_A"])
        self.assertEqual(row["numeric_result"], "RISK_STOP_REQUIRED_ARM")

    def test_technical_prefix_never_becomes_complete_inference(self):
        inputs, constants = fixture()
        inputs["invocation_status"] = "TECHNICAL_HALT"
        inputs["roles"][0]["status"] = "TECHNICAL_HALT"
        inputs["roles"][0]["daily"] = inputs["roles"][0]["daily"][:30]
        report = calculate(inputs, constants)
        self.assertEqual(report["invocation_result"], "INCOMPLETE_TECHNICAL")
        self.assertIsNone(report["families"]["shock"]["economic_H"])
        self.assertFalse(report["advance_authority"])

    def test_full_native_cash_and_paired_deltas_do_not_overflow(self):
        values = inference.deltas((inference.I64_MIN, inference.I64_MAX), inference.I64_MAX)
        self.assertEqual(values, [-(2**64 - 1), 2**64 - 1])
        self.assertGreater(values[1] - values[0], inference.I64_MAX)

    def test_large_valid_cash_keeps_exact_variance_without_i128_limit(self):
        inputs, constants = fixture()
        multiplier = 8_000_000
        for role in inputs["roles"][::2]:
            for row in role["daily"]:
                row["equity_ticks"] = C0 + (row["equity_ticks"] - C0) * multiplier
                self.assertLessEqual(row["equity_ticks"], inference.I64_MAX)
        row = calculate(inputs, constants)["families"]["shock"]["economic_H"]["by_block_days"]["56"]
        self.assertEqual(as_fraction(row["se_squared"]),
                         Fraction(5329, 7_600_000_000_000_000) * multiplier**2)
        self.assertEqual(row["precision"], "FAIL")


class BoundaryTests(unittest.TestCase):
    def test_money_boolean_float_unknown_key_and_duplicate_json_refuse(self):
        for bad in (True, 1.0, inference.I64_MAX + 1):
            inputs, _ = fixture(3)
            inputs["roles"][0]["daily"][0]["equity_ticks"] = bad
            with self.assertRaises(inference.InferenceError):
                inference.parse_cash(inputs)
        inputs, _ = fixture(3)
        inputs["extra"] = "unknown"
        with self.assertRaises(inference.InferenceError):
            inference.parse_cash(inputs)
        for raw, code in ((b'{"a":1,"a":2}', "DUPLICATE_KEY"),
                          (b'{"a":1.0}', "INVALID_TYPE"), (b'{"a":NaN}', "INVALID_TYPE"),
                          (b'{"a":' + b'1' * 4301 + b'}', "BOUND_EXCEEDED")):
            with self.assertRaises(inference.InferenceError) as caught:
                inference.decode(raw)
            self.assertEqual(caught.exception.code, code)

    def test_missing_day_missing_role_and_holdout_calendar_refuse(self):
        for mutation in ("day", "role", "holdout"):
            inputs, _ = fixture(3)
            if mutation == "day":
                inputs["roles"][0]["daily"][1]["day"] = "2020-12-20"
            elif mutation == "role":
                inputs["roles"].pop()
            else:
                inputs["start_day"] = "2025-10-01"
            with self.assertRaises(inference.InferenceError):
                inference.parse_cash(inputs)

    def test_out_of_range_start_is_typed_before_date_addition(self):
        inputs, _ = fixture()
        inputs["start_day"] = "9999-12-31"
        with self.assertRaises(inference.InferenceError) as caught:
            inference.parse_cash(inputs)
        self.assertEqual(caught.exception.code, "INVALID_CALENDAR")

    def test_critical_df_probability_source_calendar_and_scope_are_bound(self):
        inputs, constants = fixture()
        cash = inference.parse_cash(inputs)
        for mutation in ("df", "probability", "source", "days", "scope"):
            value = deepcopy(constants)
            if mutation == "df": value["brackets"][0]["df"] += 1
            elif mutation == "probability": value["probability"]["den"] = 239
            elif mutation == "source": value["source_id"] = "wrong"
            elif mutation == "days": value["days"] -= 1
            else: value["scope"] = "EXTERNAL_CERTIFICATE"
            with self.assertRaises(inference.InferenceError):
                inference.parse_critical(value, cash, SOURCE)

    def test_finite_shape_and_critical_encoding_bounds_refuse(self):
        inputs, constants = fixture(3)
        for key, bad in (("days", 1749), ("days", True), ("c0_ticks", 0)):
            changed = deepcopy(inputs)
            changed[key] = bad
            with self.assertRaises(inference.InferenceError):
                inference.parse_cash(changed)
        changed = deepcopy(inputs)
        changed["roles"].append(deepcopy(changed["roles"][0]))
        with self.assertRaises(inference.InferenceError):
            inference.parse_cash(changed)
        inputs, constants = fixture()
        constants["brackets"][0]["upper"]["num"] = "1" * 257
        with self.assertRaises(inference.InferenceError):
            inference.parse_critical(constants, inference.parse_cash(inputs), SOURCE)
        with self.assertRaises(inference.InferenceError):
            inference.rational({"num": "-0", "den": "1"})


class CliTests(unittest.TestCase):
    def test_actual_cli_digest_binding_preservation_runtime_and_no_overwrite(self):
        inputs, constants = fixture()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            cash, critical, out = root / "cash.json", root / "critical.json", root / "diagnostic.json"
            cash.write_text(json.dumps(inputs), encoding="utf-8")
            critical.write_text(json.dumps(constants), encoding="utf-8")
            original = {path: path.read_bytes() for path in (cash, critical)}
            command = [sys.executable, "-I", "-B", str(SCRIPT), "--cash", str(cash),
                       "--critical", str(critical), "--out", str(out),
                       "--expected-cash-sha256", hashlib.sha256(original[cash]).hexdigest(),
                       "--expected-critical-sha256", hashlib.sha256(original[critical]).hexdigest(),
                       "--expected-critical-source", SOURCE]
            first = subprocess.run(command, capture_output=True, text=True, check=False)
            self.assertEqual(first.returncode, 0, first.stderr)
            saved = out.read_bytes()
            result = json.loads(saved)
            self.assertEqual(result["scope"], "SYNTHETIC_NO_RUN")
            self.assertTrue(result["runtime"]["isolated"])
            self.assertTrue(result["runtime"]["bytecode_disabled"])
            self.assertFalse(result["cdf_certificate_verified_here"])
            second = subprocess.run(command, capture_output=True, text=True, check=False)
            self.assertEqual(second.returncode, 2)
            self.assertEqual(out.read_bytes(), saved)
            altered = command.copy()
            altered[altered.index("--out") + 1] = str(root / "refused.json")
            altered[altered.index("--expected-critical-sha256") + 1] = "0" * 64
            refused = subprocess.run(altered, capture_output=True, text=True, check=False)
            self.assertEqual(refused.returncode, 2)
            self.assertEqual(json.loads(refused.stderr)["error"], "DIGEST_MISMATCH")
            self.assertFalse((root / "refused.json").exists())
            self.assertEqual({path: path.read_bytes() for path in (cash, critical)}, original)
            malformed = deepcopy(inputs)
            malformed["start_day"] = "9999-12-31"
            bad_cash = root / "invalid-calendar.json"
            bad_cash.write_text(json.dumps(malformed), encoding="utf-8")
            bad = command.copy()
            bad[bad.index("--cash") + 1] = str(bad_cash)
            bad[bad.index("--expected-cash-sha256") + 1] = hashlib.sha256(bad_cash.read_bytes()).hexdigest()
            bad[bad.index("--out") + 1] = str(root / "invalid-calendar-output.json")
            refused = subprocess.run(bad, capture_output=True, text=True, check=False)
            self.assertEqual(refused.returncode, 2)
            self.assertEqual(json.loads(refused.stderr)["error"], "INVALID_CALENDAR")
            self.assertFalse((root / "invalid-calendar-output.json").exists())


if __name__ == "__main__":
    unittest.main(verbosity=2)
