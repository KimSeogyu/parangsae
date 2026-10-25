#!/usr/bin/env python3
"""Exact, read-only Astra cash diagnostics; no simulator or custody authority.

Python 3.11+, standard library only. Actual critical enclosures and native
producer/ledger custody require separate preflight. Synthetic inputs are NOT RUNS.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from datetime import date, timedelta
from fractions import Fraction
import hashlib
import json
from math import isqrt
import os
from pathlib import Path
import platform
import re
import sys


MAX_DAYS = 1748
MAX_CASH_BYTES = 4_000_000
MAX_CRITICAL_BYTES = 64_000
I64_MIN, I64_MAX = -(1 << 63), (1 << 63) - 1
FIRST_DAY, HOLDOUT = date(2020, 12, 18), date(2025, 10, 1)
BLOCKS = (56, 28)
PRECISION = Fraction(1, 50)
HURDLE = Fraction(1, 50)
DISPLAY_SCALE = 10**30
FAMILIES = {
    "shock": ("shock_primary", "shock_direction_control"),
    "range": ("range_primary", "range_direction_control"),
    "premium": ("premium_primary", "premium_price_control"),
}
REQUIRED = frozenset(role for pair in FAMILIES.values() for role in pair)
ALL_ROLES = REQUIRED | frozenset(f"{name}_long_reference" for name in FAMILIES)
STATUSES = {"COMPLETE", "RISK_STOP", "TECHNICAL_HALT", "NOT_STARTED"}


class InferenceError(ValueError):
    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code


def require(condition: bool, code: str, message: str) -> None:
    if not condition:
        raise InferenceError(code, message)


def fields(value: object, expected: set[str], label: str) -> dict:
    require(type(value) is dict, "INVALID_TYPE", f"{label} must be an object")
    require(set(value) == expected, "INVALID_FIELDS", f"{label} fields differ")
    return value


def integer(value: object, low: int, high: int, label: str) -> int:
    require(type(value) is int, "INVALID_TYPE", f"{label} must be an integer")
    require(low <= value <= high, "BOUND_EXCEEDED", f"{label} exceeds its bound")
    return value


def civil_day(value: object) -> date:
    require(type(value) is str, "INVALID_CALENDAR", "day must be ISO text")
    try:
        parsed = date.fromisoformat(value)
    except ValueError as error:
        raise InferenceError("INVALID_CALENDAR", "invalid UTC day") from error
    require(parsed.isoformat() == value, "INVALID_CALENDAR", "day must be YYYY-MM-DD")
    return parsed


def hex_digest(value: object) -> str:
    require(type(value) is str and re.fullmatch(r"[0-9a-f]{64}", value) is not None,
            "INVALID_DIGEST", "expected a lowercase SHA256")
    return value


def rational(value: object) -> Fraction:
    obj = fields(value, {"num", "den"}, "rational")
    for part in (obj["num"], obj["den"]):
        require(type(part) is str and len(part) <= 256
                and re.fullmatch(r"0|-?[1-9][0-9]*", part) is not None,
                "INVALID_RATIONAL", "bounded canonical integer strings required")
    num, den = int(obj["num"]), int(obj["den"])
    require(den > 0, "INVALID_RATIONAL", "rational denominator must be positive")
    return Fraction(num, den)


def ratio(value: Fraction) -> dict[str, str]:
    return {"num": str(value.numerator), "den": str(value.denominator)}


def unique_object(pairs: list[tuple[str, object]]) -> dict:
    result = {}
    for key, value in pairs:
        require(key not in result, "DUPLICATE_KEY", "duplicate JSON object key")
        result[key] = value
    return result


def reject_number(_: str) -> None:
    raise InferenceError("INVALID_TYPE", "JSON floats and nonfinite numbers are forbidden")


def bounded_json_integer(token: str) -> int:
    require(len(token) <= 20, "BOUND_EXCEEDED", "JSON integer token exceeds native cash width")
    return int(token)


def decode(raw: bytes) -> dict:
    try:
        return json.loads(raw, object_pairs_hook=unique_object,
                          parse_int=bounded_json_integer,
                          parse_float=reject_number, parse_constant=reject_number)
    except (UnicodeDecodeError, json.JSONDecodeError, RecursionError) as error:
        raise InferenceError("INVALID_JSON", "invalid bounded UTF-8 JSON") from error


@dataclass(frozen=True)
class Role:
    status: str
    exposed: bool
    equity: tuple[int, ...]


@dataclass(frozen=True)
class CashEvidence:
    scope: str
    start: date
    days: int
    c0: int
    invocation_status: str
    roles: dict[str, Role]


@dataclass(frozen=True)
class CriticalEvidence:
    scope: str
    source: str
    certificate_digest: str
    brackets: dict[int, tuple[Fraction, Fraction]]


def parse_cash(value: object) -> CashEvidence:
    obj = fields(value, {"schema", "scope", "start_day", "days", "c0_ticks",
                         "invocation_status", "roles"}, "cash evidence")
    require(obj["schema"] == "astra-cash-evidence-v1", "INVALID_SCHEMA", "cash schema differs")
    require(obj["scope"] in ("SYNTHETIC_NO_RUN", "RUST_EXPORT_UNVERIFIED"),
            "INVALID_SCOPE", "cash scope differs")
    start = civil_day(obj["start_day"])
    days = integer(obj["days"], 1, MAX_DAYS, "calendar days")
    require(FIRST_DAY <= start and start + timedelta(days=days) <= HOLDOUT,
            "INVALID_CALENDAR", "calendar crosses allowed development bounds")
    c0 = integer(obj["c0_ticks"], 1, I64_MAX, "original C0 cash ticks")
    require(obj["invocation_status"] in ("COMPLETE", "TECHNICAL_HALT"),
            "INVALID_STATUS", "invocation status differs")
    require(type(obj["roles"]) is list and 6 <= len(obj["roles"]) <= 9,
            "INVALID_ROLES", "six required and at most nine roles must be declared")
    roles = {}
    for item in obj["roles"]:
        row = fields(item, {"role", "status", "exposed", "daily"}, "role")
        name = row["role"]
        require(type(name) is str and name in ALL_ROLES and name not in roles,
                "INVALID_ROLES", "unknown or duplicate role")
        require(type(row["status"]) is str and row["status"] in STATUSES,
                "INVALID_STATUS", "role status differs")
        require(type(row["exposed"]) is bool, "INVALID_TYPE", "exposed must be a boolean")
        daily = row["daily"]
        require(type(daily) is list and len(daily) <= days,
                "INVALID_CALENDAR", "role daily length exceeds calendar")
        require(row["status"] != "COMPLETE" or len(daily) == days,
                "INVALID_CALENDAR", "complete role must retain every calendar day")
        require(row["status"] != "NOT_STARTED" or (not daily and not row["exposed"]),
                "INVALID_STATUS", "not-started role has observations")
        equity = []
        for index, day_row in enumerate(daily):
            entry = fields(day_row, {"day", "equity_ticks"}, "daily cash")
            require(civil_day(entry["day"]) == start + timedelta(days=index),
                    "INVALID_CALENDAR", "daily calendar is missing, duplicated or unordered")
            equity.append(integer(entry["equity_ticks"], I64_MIN, I64_MAX, "cash equity ticks"))
        require(row["exposed"] or all(level == c0 for level in equity),
                "INVALID_STATUS", "declared cash-only path changes equity")
        roles[name] = Role(row["status"], row["exposed"], tuple(equity))
    require(REQUIRED <= roles.keys(), "INVALID_ROLES", "required role is absent")
    technical = any(role.status == "TECHNICAL_HALT" for role in roles.values())
    require(technical == (obj["invocation_status"] == "TECHNICAL_HALT"),
            "INVALID_STATUS", "invocation and technical role statuses disagree")
    return CashEvidence(obj["scope"], start, days, c0, obj["invocation_status"], roles)


def parse_critical(value: object, cash: CashEvidence, expected_source: str) -> CriticalEvidence:
    obj = fields(value, {"schema", "scope", "start_day", "days", "source_id",
                         "certificate_sha256", "probability", "brackets"}, "critical evidence")
    require(obj["schema"] == "astra-critical-enclosures-v1",
            "INVALID_SCHEMA", "critical schema differs")
    expected_scope = "SYNTHETIC_NO_RUN" if cash.scope == "SYNTHETIC_NO_RUN" else "EXTERNAL_CERTIFICATE"
    require(obj["scope"] == expected_scope, "INVALID_SCOPE", "critical and cash scopes disagree")
    require(civil_day(obj["start_day"]) == cash.start and type(obj["days"]) is int
            and obj["days"] == cash.days, "CRITICAL_BINDING", "critical calendar differs")
    require(type(obj["source_id"]) is str and 0 < len(obj["source_id"]) <= 256
            and obj["source_id"] == expected_source,
            "CRITICAL_BINDING", "critical source differs from external expected source")
    certificate = hex_digest(obj["certificate_sha256"])
    probability = fields(obj["probability"], {"num", "den"}, "critical probability")
    require(type(probability["num"]) is int and type(probability["den"]) is int
            and (probability["num"], probability["den"]) == (239, 240),
            "CRITICAL_BINDING", "critical probability must be exactly 239/240")
    require(type(obj["brackets"]) is list, "INVALID_TYPE", "critical brackets must be a list")
    eligible = cash.days // 56 >= 20
    require(len(obj["brackets"]) == (2 if eligible else 0),
            "CRITICAL_BINDING", "critical pair must match interval eligibility")
    brackets = {}
    for item in obj["brackets"]:
        row = fields(item, {"block_days", "df", "lower", "upper"}, "critical bracket")
        length = row["block_days"]
        require(type(length) is int and length in BLOCKS and length not in brackets,
                "CRITICAL_BINDING", "critical block length differs")
        require(type(row["df"]) is int and row["df"] == cash.days // length - 1,
                "CRITICAL_BINDING", "critical degrees of freedom differ")
        low, high = rational(row["lower"]), rational(row["upper"])
        require(0 < low <= high, "INVALID_RATIONAL", "critical enclosure must be positive and ordered")
        brackets[length] = (low, high)
    return CriticalEvidence(obj["scope"], expected_source, certificate, brackets)


def deltas(equity: tuple[int, ...], c0: int) -> list[int]:
    # Python integers widen before either subtraction; no i64 delta truncation.
    return [current - previous for previous, current in zip((c0,) + equity, equity)]


def sqrt_enclosure(value: Fraction) -> tuple[Fraction, Fraction]:
    require(value >= 0, "NUMERIC_UNDEFINED", "negative squared half-width")
    scaled = value.numerator * DISPLAY_SCALE**2
    root = isqrt(scaled // value.denominator)
    lower = Fraction(root, DISPLAY_SCALE)
    exact = root * root * value.denominator == scaled
    return lower, lower if exact else Fraction(root + 1, DISPLAY_SCALE)


def classify_squared(point: Fraction, lower2: Fraction, upper2: Fraction) -> tuple[str, str]:
    """Classify only when every value in the certified width enclosure agrees."""
    if point > 0:
        status = ("SUPPORT" if point * point > upper2 else
                  "INSUFFICIENT" if point * point <= lower2 else "NUMERIC_UNDEFINED")
    elif point < 0:
        status = ("FAILURE" if point * point >= upper2 else
                  "INSUFFICIENT" if point * point < lower2 else "NUMERIC_UNDEFINED")
    else:
        status = "INSUFFICIENT"
    precision = ("PASS" if upper2 <= PRECISION**2 else
                 "FAIL" if lower2 > PRECISION**2 else "NUMERIC_UNDEFINED")
    return status, precision


def contrast(values: list[int], c0: int, center: Fraction, exposed: bool,
             critical: CriticalEvidence) -> dict:
    days, total = len(values), sum(values)
    point = Fraction(365 * total, days * c0) - center
    output = {"point": ratio(point), "cash_sum_ticks": str(total), "by_block_days": {}}
    for length in BLOCKS:
        base = {"point": ratio(point), "precision": "UNAVAILABLE"}
        if days // 56 < 20:
            output["by_block_days"][str(length)] = base | {"status": "INELIGIBLE"}
            continue
        if not exposed:
            output["by_block_days"][str(length)] = base | {"status": "UNDEFINED_NO_EXPOSURE"}
            continue
        blocks = days // length
        boundaries = [index * days // blocks for index in range(blocks + 1)]
        numerators = [days * sum(values[left:right]) - (right - left) * total
                      for left, right in zip(boundaries, boundaries[1:])]
        se2 = Fraction(365**2 * blocks * sum(value * value for value in numerators),
                       (blocks - 1) * days**4 * c0**2)
        base |= {"blocks": blocks, "boundaries": boundaries, "se_squared": ratio(se2)}
        if se2 == 0:
            output["by_block_days"][str(length)] = base | {"status": "UNDEFINED_ZERO_SE"}
            continue
        low, high = critical.brackets[length]
        low2, high2 = low * low * se2, high * high * se2
        status, precision = classify_squared(point, low2, high2)
        width_low, _ = sqrt_enclosure(low2)
        _, width_high = sqrt_enclosure(high2)
        output["by_block_days"][str(length)] = base | {
            "status": status, "precision": precision,
            "half_width_squared": {"lower": ratio(low2), "upper": ratio(high2)},
            "outward_endpoint_display": {"lower": ratio(point - width_high),
                                         "upper": ratio(point + width_high)},
            "half_width_display": {"lower": ratio(width_low), "upper": ratio(width_high)},
            "classification_uses": "EXACT_SQUARED_RATIONALS_NOT_DISPLAY_ROUNDING",
        }
    return output


def family_result(primary: Role, control: Role, h: dict | None, d: dict | None) -> str:
    if primary.status == "RISK_STOP" or control.status == "RISK_STOP":
        return "RISK_STOP_REQUIRED_ARM"
    if h is None or d is None:
        return "REQUIRED_ARM_UNAVAILABLE"
    if int(h["cash_sum_ticks"]) < 0:
        return "OBSERVED_PRIMARY_CASH_LOSS"
    rows = [item["by_block_days"] for item in (h, d)]
    for name, item in zip(("ECONOMIC", "DIRECTION"), rows):
        if all(item[str(length)]["status"] == "FAILURE" for length in BLOCKS):
            return f"{name}_ENDPOINT_FAILURE"
    cells = [item[str(length)] for item in rows for length in BLOCKS]
    if all(cell["status"] == "SUPPORT" and cell["precision"] == "PASS" for cell in cells):
        return "NUMERIC_LOCAL_SUPPORT"
    if any(cell["status"] == "NUMERIC_UNDEFINED" or cell["precision"] == "NUMERIC_UNDEFINED"
           for cell in cells):
        return "NUMERIC_UNDEFINED"
    if any(cell["status"].startswith("UNDEFINED") or cell["status"] == "INELIGIBLE" for cell in cells):
        return "INSUFFICIENT_INFORMATION"
    if any(item["56"]["status"] != item["28"]["status"] for item in rows):
        return "INCONCLUSIVE_DEPENDENCE"
    return "INSUFFICIENT_INFORMATION"


def analyze(cash: CashEvidence, critical: CriticalEvidence) -> dict:
    families = {}
    for name, (primary_name, control_name) in FAMILIES.items():
        primary, control = cash.roles[primary_name], cash.roles[control_name]
        h = d = a = None
        if primary.status == "COMPLETE":
            values = deltas(primary.equity, cash.c0)
            a = ratio(Fraction(365 * sum(values), cash.days * cash.c0))
            h = contrast(values, cash.c0, HURDLE, primary.exposed, critical)
            if control.status == "COMPLETE":
                other = deltas(control.equity, cash.c0)
                paired = [left - right for left, right in zip(values, other)]
                d = contrast(paired, cash.c0, Fraction(0), primary.exposed, critical)
        families[name] = {"annual_simple_A": a, "economic_H": h, "direction_D": d,
                          "numeric_result": family_result(primary, control, h, d)}
    return {
        "schema": "astra-numeric-diagnostics-v1", "scope": cash.scope,
        "days": cash.days, "c0_ticks": str(cash.c0), "families": families,
        "role_outcomes": {name: {"status": role.status, "observed_days": len(role.equity)}
                          for name, role in cash.roles.items()},
        "invocation_result": "INCOMPLETE_TECHNICAL" if cash.invocation_status == "TECHNICAL_HALT"
                             else "NUMERIC_DIAGNOSTICS_ONLY",
        "advance_authority": False, "producer_custody_verified_here": False,
        "cdf_certificate_verified_here": False,
        "critical_source": critical.source, "critical_certificate_sha256": critical.certificate_digest,
        "external_preflight_required": True,
        "favorable_verdict_ceiling": "INCONCLUSIVE_HISTORY",
    }


def read_bound(path: Path, limit: int, expected: str) -> bytes:
    expected = hex_digest(expected)
    with path.open("rb") as source:
        raw = source.read(limit + 1)
    require(len(raw) <= limit, "BOUND_EXCEEDED", "input file exceeds size bound")
    require(hashlib.sha256(raw).hexdigest() == expected, "DIGEST_MISMATCH", "input digest differs")
    return raw


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cash", type=Path, required=True)
    parser.add_argument("--critical", type=Path, required=True)
    parser.add_argument("--expected-cash-sha256", required=True)
    parser.add_argument("--expected-critical-sha256", required=True)
    parser.add_argument("--expected-critical-source", required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        require(sys.version_info >= (3, 11), "UNSUPPORTED_PYTHON", "Python 3.11 or newer is required")
        raw = read_bound(args.cash, MAX_CASH_BYTES, args.expected_cash_sha256)
        cash = parse_cash(decode(raw))
        constants = read_bound(args.critical, MAX_CRITICAL_BYTES, args.expected_critical_sha256)
        critical = parse_critical(decode(constants), cash, args.expected_critical_source)
        result = analyze(cash, critical)
        result["input_sha256"] = hashlib.sha256(raw).hexdigest()
        result["critical_sha256"] = hashlib.sha256(constants).hexdigest()
        result["runtime"] = {"version": platform.python_version(), "executable": sys.executable,
                             "isolated": bool(sys.flags.isolated),
                             "bytecode_disabled": sys.dont_write_bytecode}
        encoded = json.dumps(result, ensure_ascii=True, sort_keys=True, separators=(",", ":")) + "\n"
        with args.out.open("x", encoding="utf-8") as target:
            target.write(encoded)
            target.flush()
            os.fsync(target.fileno())
        return 0
    except InferenceError as error:
        print(json.dumps({"error": error.code, "message": str(error)}), file=sys.stderr)
    except (OSError, ValueError) as error:
        print(json.dumps({"error": "IO_OR_ENCODING_ERROR", "message": str(error)}), file=sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
