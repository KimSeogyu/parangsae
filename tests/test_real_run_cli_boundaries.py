from __future__ import annotations

import io
import os
import zipfile
from argparse import Namespace
from pathlib import Path

import pandas as pd

from parangsae.apps.cli import quantctl
from parangsae.marketdata.binance_funding import BINANCE_USDM_FUNDING_RATE_URL
from parangsae.marketdata.binance_vision import CANONICAL_MONTHS, canonical_urls
from parangsae.research.readiness import (
    READINESS_ABLATION_COLUMNS,
    READINESS_JSON_TOP_LEVEL_KEYS,
    READINESS_REGIME_COLUMNS,
    READINESS_SCHEMA_VERSION,
    READINESS_WINDOWS_COLUMNS,
    ReadinessReport,
)


def zip_payload(rows: int = 120) -> bytes:
    base = 1704067200000
    csv = io.StringIO()
    for i in range(rows):
        ts = base + i * 3600000
        price = 100 + (i % 20)
        csv.write(f"{ts},{price},{price+2},{price-2},{price+1},10,{ts+3599999},0,1,0,0,0\n")
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as zf:
        zf.writestr("BTCUSDT-1h-2024-01.csv", csv.getvalue())
    return buf.getvalue()


def canonical_month_zip_payload(url: str) -> bytes:
    stem = Path(url).name.removesuffix(".zip")
    parts = stem.rsplit("-", 2)
    month = f"{parts[-2]}-{parts[-1]}"
    start = pd.Timestamp(f"{month}-01T00:00:00Z")
    end = start + pd.offsets.MonthBegin(1)
    csv = io.StringIO()
    for i, timestamp in enumerate(pd.date_range(start, end, freq="1h", inclusive="left")):
        ts = int(timestamp.timestamp() * 1000)
        price = 100 + i * 0.01
        csv.write(f"{ts},{price},{price+2},{price-2},{price+1},10,{ts+3599999},0,1,0,0,0\n")
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as zf:
        zf.writestr(f"BTCUSDT-1h-{month}.csv", csv.getvalue())
    return buf.getvalue()


class FakeResponse:
    def __init__(self, content: bytes = b"", json_data=None):
        self.content = content
        self._json_data = json_data
        self.status_code = 200
        self.headers = {"ETag": "fixture", "Last-Modified": "fixture"}

    def raise_for_status(self):
        return None

    def json(self):
        return self._json_data


def args(tmp_path: Path) -> Namespace:
    return Namespace(
        symbol="BTCUSDT",
        interval="1h",
        months=",".join(CANONICAL_MONTHS),
        cache_dir=str(tmp_path / "cache"),
        reports_root=str(tmp_path / "reports"),
        output_dir=None,
        overwrite=False,
        product_family="spot_crypto",
        funding_source="public-binance",
        funding_cache_dir=str(tmp_path / "funding-cache"),
    )


def funding_rows() -> list[dict[str, str | int]]:
    base = pd.Timestamp("2024-01-01T00:00:00Z")
    return [
        {
            "symbol": "BTCUSDT",
            "fundingRate": "0.00010000",
            "fundingTime": int((base + pd.Timedelta(hours=8 * i)).timestamp() * 1000),
            "markPrice": "43000.0",
        }
        for i in range(550)
    ]


def fake_readiness_report(frame, **kwargs) -> ReadinessReport:
    del frame, kwargs
    windows = pd.DataFrame(
        [
            {
                "window_id": "W001",
                "train_start": "2024-01-01T00:00:00Z",
                "train_end": "2024-01-02T00:00:00Z",
                "validation_start": "2024-01-02T01:00:00Z",
                "validation_end": "2024-01-03T00:00:00Z",
                "holdout_start": "2024-01-03T01:00:00Z",
                "holdout_end": "2024-01-04T00:00:00Z",
                "selected_candidate_id": "c001",
                "selected_fast_sma": 8,
                "selected_slow_sma": 21,
                "selected_risk_fraction": 0.5,
                "train_score": 1.0,
                "validation_score": 1.0,
                "base_holdout_return_pct": 1.0,
                "base_holdout_sharpe": 1.0,
                "base_holdout_max_drawdown_pct": -1.0,
                "base_holdout_closed_trades": 2,
                "base_buy_hold_return_pct": 0.5,
                "cash_null_return_pct": 0.0,
                "stress_holdout_return_pct": -0.5,
                "stress_holdout_sharpe": 0.5,
                "stress_holdout_max_drawdown_pct": -2.0,
                "stress_funding_rate_per_8h": 0.0001,
                "stress_funding_drag_pct": 0.01,
                "stress_slippage_proxy_bps": 5.0,
                "beats_cash_null": False,
                "beats_buy_hold": False,
                "stress_survived": True,
                "verdict": "FAIL",
            }
        ],
        columns=READINESS_WINDOWS_COLUMNS,
    )
    ablation = pd.DataFrame(columns=READINESS_ABLATION_COLUMNS)
    regime = pd.DataFrame(columns=READINESS_REGIME_COLUMNS)
    compute_profile = {
        "schema_version": 1,
        "profiled_at": "2024-01-01T00:00:00Z",
        "elapsed_seconds": 0.01,
        "elapsed_seconds_role": "diagnostic only; fixture speed is not a research-grade proof",
        "engine": "fixture",
        "total_input_bars": 1000,
        "input_start": "2024-01-01T00:00:00Z",
        "input_end": "2024-02-11T15:00:00Z",
        "input_span_days": 41.625,
        "window_count": 1,
        "candidate_count": 3,
        "candidate_families": ["sma_atr_bracket", "alpha_signal_factory_regime"],
        "candidate_window_evaluations": 3,
        "candidate_selection_evaluations": 6,
        "selected_holdout_evaluations": 2,
        "total_strategy_evaluations": 8,
        "bars_processed_estimate": 1000,
        "bars_processed_formula": "fixture",
        "profile_notes": ["fixture"],
    }
    validation_depth = {
        "schema_version": 1,
        "grade": "prototype",
        "grade_order": ["smoke", "prototype", "research_grade"],
        "research_grade": False,
        "gate": "validation_depth_research_grade",
        "observed": {
            "total_input_bars": 1000,
            "window_count": 1,
            "candidate_count": 3,
            "candidate_window_evaluations": 3,
            "bars_processed_estimate": 1000,
        },
        "thresholds": {
            "total_input_bars": 17520,
            "window_count": 12,
            "candidate_count": 25,
            "candidate_window_evaluations": 300,
            "bars_processed_estimate": 500000,
        },
        "failed_depth_checks": [
            "total_input_bars",
            "window_count",
            "candidate_count",
            "candidate_window_evaluations",
            "bars_processed_estimate",
        ],
        "classification_reason": "prototype coverage only; fixture is intentionally shallow",
        "wall_clock_policy": "elapsed_seconds is diagnostic only and never upgraded to a readiness pass/fail criterion",
    }
    payload = {
        "schema_version": READINESS_SCHEMA_VERSION,
        "method": {
            "selection_uses_holdout": False,
            "live_orders_submitted": False,
            "api_keys_required": False,
            "validation_depth_policy": "elapsed_seconds is diagnostic only",
            "data_coverage": {},
        },
        "config": {},
        "artifact_schemas": {
            "readiness_windows.csv": READINESS_WINDOWS_COLUMNS,
            "readiness_ablation.csv": READINESS_ABLATION_COLUMNS,
            "readiness_regime.csv": READINESS_REGIME_COLUMNS,
            "readiness.json": READINESS_JSON_TOP_LEVEL_KEYS,
        },
        "compute_profile": compute_profile,
        "validation_depth": validation_depth,
        "cost_scenarios": {
            "stress": {
                "commission": 0.001,
                "spread": 0.001,
                "slippage_proxy_bps": 5.0,
                "slippage_formula": "closed_trades * 2 * slippage_proxy_bps / 100 percentage points",
            }
        },
        "funding_scenarios": {
            "stress_funding_rate_per_8h": 0.0001,
            "funding_formula": "exposure_time_pct / 100 * bars / 8 * funding_rate_per_8h * 100",
        },
        "summary": {
            "verdict": "NOT_READY",
            "failed_gates": ["median_stress_return_beats_cash_null", "validation_depth_research_grade"],
            "gate_thresholds": {
                "min_windows": 3,
                "min_holdout_trades_each": 1,
                "max_high_corr_family_pairs": 6,
                "max_selected_candidate_switch_rate": 0.75,
                "min_regime_labels": 2,
                "stress_return_floor_pct": -15.0,
                "validation_depth_research_grade": validation_depth["thresholds"],
            },
            "window_count": 1,
            "median_stress_holdout_return_pct": -0.5,
            "median_base_holdout_return_pct": 1.0,
            "all_holdouts_have_trades": True,
            "regime_label_count": 0,
            "selected_candidate_switch_rate": 0.0,
            "lookahead_passed": True,
        },
        "walk_forward": {"window_count": 1, "rows": windows.to_dict(orient="records")},
        "baselines": {"cash_null_return_pct": 0.0, "buy_hold_source": "backtesting_metric"},
        "ablation": {"method": "fixture", "rows": []},
        "duplicate_risk": {
            "max_raw_signals_per_family_per_timestamp": 0,
            "high_corr_family_pair_count": 0,
            "high_corr_threshold": 0.8,
            "high_corr_pairs": [],
        },
        "regime_split": {"regime_label_count": 0, "rows": []},
        "stress": {"stress_return_floor_pct": -15.0, "all_windows_survived": True},
        "lookahead_checks": {"status": "passed", "passed": True},
        "overfit_checks": {"status": "passed", "selected_candidate_switch_rate": 0.0},
        "live_blockers": ["No live/testnet order path was validated or authorized."],
        "claim_boundary": "readiness fixture is not live-ready",
    }
    return ReadinessReport(windows, ablation, regime, payload)


def test_real_run_only_uses_canonical_binance_vision_gets(monkeypatch, tmp_path):
    allowed = set(canonical_urls("BTCUSDT", "1h", CANONICAL_MONTHS))
    calls = []

    def fake_request(self, method, url, **kwargs):
        calls.append((method, url))
        assert method == "GET"
        assert url in allowed
        return FakeResponse(zip_payload())

    monkeypatch.setattr("requests.sessions.Session.request", fake_request)
    rc = quantctl.cmd_real_run(args(tmp_path))
    assert rc == 2  # fixture is real-shaped but intentionally below strict bars/trades gates
    assert calls == [("GET", url) for url in canonical_urls("BTCUSDT", "1h", CANONICAL_MONTHS)]
    report_dirs = list((tmp_path / "reports").iterdir())
    assert len(report_dirs) == 1
    assert (report_dirs[0] / "failure.json").exists()
    assert not (report_dirs[0] / "report.md").exists()


def test_real_run_does_not_read_binance_credentials_or_import_ccxt(monkeypatch, tmp_path):
    monkeypatch.setenv("BINANCE_API_KEY", "SHOULD_NOT_BE_READ")
    monkeypatch.setenv("BINANCE_API_SECRET", "SHOULD_NOT_BE_READ")
    real_getenv = os.getenv

    def guarded_getenv(key, default=None):
        if key in {"BINANCE_API_KEY", "BINANCE_API_SECRET"}:
            raise AssertionError(f"real-run must not read {key}")
        return real_getenv(key, default)

    monkeypatch.setattr(os, "getenv", guarded_getenv)
    monkeypatch.setitem(__import__("sys").modules, "ccxt", None)
    monkeypatch.setattr("requests.sessions.Session.request", lambda self, method, url, **kwargs: FakeResponse(zip_payload()))
    assert quantctl.cmd_real_run(args(tmp_path)) == 2


def test_real_run_static_source_has_no_order_or_credential_path():
    source = Path("src/parangsae/apps/cli/quantctl.py").read_text()
    real_run_source = source[source.index("def cmd_real_run") : source.index("def cmd_kill_switch")]
    assert "ccxt" not in real_run_source
    assert "create_order" not in real_run_source
    assert "BINANCE_API_KEY" not in real_run_source
    assert "BINANCE_API_SECRET" not in real_run_source
    readiness_source = source[source.index("def cmd_readiness_run") : source.index("def cmd_kill_switch")]
    assert "ccxt" not in readiness_source
    assert "create_order" not in readiness_source
    assert "BINANCE_API_KEY" not in readiness_source
    assert "BINANCE_API_SECRET" not in readiness_source


def test_real_run_rejects_noncanonical_window_with_failure_artifact(tmp_path):
    a = args(tmp_path)
    a.months = "2024-02"
    assert quantctl.cmd_real_run(a) == 2
    report_dirs = list((tmp_path / "reports").iterdir())
    assert len(report_dirs) == 1
    assert (report_dirs[0] / "failure.json").exists()
    assert not (report_dirs[0] / "report.md").exists()


def test_risk_run_only_uses_canonical_binance_vision_gets(monkeypatch, tmp_path):
    allowed = set(canonical_urls("BTCUSDT", "1h", CANONICAL_MONTHS))
    calls = []

    def fake_request(self, method, url, **kwargs):
        calls.append((method, url))
        assert method == "GET"
        assert url in allowed
        return FakeResponse(zip_payload())

    monkeypatch.setattr("requests.sessions.Session.request", fake_request)
    rc = quantctl.cmd_risk_run(args(tmp_path))
    assert rc == 2
    assert calls == [("GET", url) for url in canonical_urls("BTCUSDT", "1h", CANONICAL_MONTHS)]
    report_dirs = list((tmp_path / "reports").iterdir())
    assert len(report_dirs) == 1
    assert report_dirs[0].name.startswith("risk-run-")
    assert (report_dirs[0] / "failure.json").exists()
    assert not (report_dirs[0] / "report.md").exists()


def test_risk_run_does_not_read_binance_credentials_or_import_ccxt(monkeypatch, tmp_path):
    monkeypatch.setenv("BINANCE_API_KEY", "SHOULD_NOT_BE_READ")
    monkeypatch.setenv("BINANCE_API_SECRET", "SHOULD_NOT_BE_READ")
    real_getenv = os.getenv

    def guarded_getenv(key, default=None):
        if key in {"BINANCE_API_KEY", "BINANCE_API_SECRET"}:
            raise AssertionError(f"risk-run must not read {key}")
        return real_getenv(key, default)

    monkeypatch.setattr(os, "getenv", guarded_getenv)
    monkeypatch.setitem(__import__("sys").modules, "ccxt", None)
    monkeypatch.setattr("requests.sessions.Session.request", lambda self, method, url, **kwargs: FakeResponse(zip_payload()))
    assert quantctl.cmd_risk_run(args(tmp_path)) == 2


def test_risk_run_invalid_product_family_writes_failure_artifact(tmp_path, capsys):
    a = args(tmp_path)
    a.product_family = "not_a_family"
    assert quantctl.cmd_risk_run(a) == 2
    captured = capsys.readouterr().out
    assert "not_a_family" in captured
    assert "Traceback" not in captured
    report_dirs = list((tmp_path / "reports").iterdir())
    assert len(report_dirs) == 1
    assert (report_dirs[0] / "failure.json").exists()
    assert not (report_dirs[0] / "report.md").exists()


def test_readiness_run_only_uses_public_market_data_gets(monkeypatch, tmp_path):
    allowed = set(canonical_urls("BTCUSDT", "1h", CANONICAL_MONTHS))
    calls = []

    def fake_request(self, method, url, **kwargs):
        calls.append((method, url))
        assert method == "GET"
        assert url in allowed | {BINANCE_USDM_FUNDING_RATE_URL}
        if url == BINANCE_USDM_FUNDING_RATE_URL:
            assert kwargs["params"]["symbol"] == "BTCUSDT"
            return FakeResponse(json_data=funding_rows())
        return FakeResponse(canonical_month_zip_payload(url))

    monkeypatch.setattr("requests.sessions.Session.request", fake_request)
    monkeypatch.setattr(quantctl, "build_readiness_report", fake_readiness_report)
    rc = quantctl.cmd_readiness_run(args(tmp_path))
    assert rc == 0
    assert calls == [("GET", url) for url in canonical_urls("BTCUSDT", "1h", CANONICAL_MONTHS)] + [
        ("GET", BINANCE_USDM_FUNDING_RATE_URL)
    ]
    report_dirs = list((tmp_path / "reports").iterdir())
    assert len(report_dirs) == 1
    assert report_dirs[0].name.startswith("readiness-run-")
    assert (report_dirs[0] / "readiness.json").exists()
    assert (report_dirs[0] / "report.md").exists()


def test_readiness_run_does_not_read_binance_credentials_or_import_ccxt(monkeypatch, tmp_path):
    monkeypatch.setenv("BINANCE_API_KEY", "SHOULD_NOT_BE_READ")
    monkeypatch.setenv("BINANCE_API_SECRET", "SHOULD_NOT_BE_READ")
    real_getenv = os.getenv

    def guarded_getenv(key, default=None):
        if key in {"BINANCE_API_KEY", "BINANCE_API_SECRET"}:
            raise AssertionError(f"readiness-run must not read {key}")
        return real_getenv(key, default)

    monkeypatch.setattr(os, "getenv", guarded_getenv)
    monkeypatch.setitem(__import__("sys").modules, "ccxt", None)
    def fake_request(self, method, url, **kwargs):
        if url == BINANCE_USDM_FUNDING_RATE_URL:
            return FakeResponse(json_data=funding_rows())
        return FakeResponse(canonical_month_zip_payload(url))

    monkeypatch.setattr("requests.sessions.Session.request", fake_request)
    monkeypatch.setattr(quantctl, "build_readiness_report", fake_readiness_report)
    assert quantctl.cmd_readiness_run(args(tmp_path)) == 0


def test_readiness_run_public_funding_unavailable_writes_failure_artifact(monkeypatch, tmp_path):
    def fake_request(self, method, url, **kwargs):
        if url == BINANCE_USDM_FUNDING_RATE_URL:
            return FakeResponse(json_data=[])
        return FakeResponse(canonical_month_zip_payload(url))

    monkeypatch.setattr("requests.sessions.Session.request", fake_request)
    a = args(tmp_path)
    assert quantctl.cmd_readiness_run(a) == 2
    report_dirs = list((tmp_path / "reports").iterdir())
    assert len(report_dirs) == 1
    assert (report_dirs[0] / "failure.json").exists()
    assert not (report_dirs[0] / "readiness.json").exists()
    failure = (report_dirs[0] / "failure.json").read_text()
    assert "no public Binance funding rows" in failure


def test_readiness_run_rejects_noncanonical_window_with_failure_artifact(tmp_path):
    a = args(tmp_path)
    a.months = "2024-02"
    assert quantctl.cmd_readiness_run(a) == 2
    report_dirs = list((tmp_path / "reports").iterdir())
    assert len(report_dirs) == 1
    assert report_dirs[0].name.startswith("readiness-run-")
    assert (report_dirs[0] / "failure.json").exists()
    assert not (report_dirs[0] / "readiness.json").exists()
    assert not (report_dirs[0] / "report.md").exists()


def test_real_run_explicit_output_collision_returns_failed_json_without_traceback(tmp_path, capsys):
    out = tmp_path / "exists"
    out.mkdir()
    a = args(tmp_path)
    a.output_dir = str(out)
    assert quantctl.cmd_real_run(a) == 2
    captured = capsys.readouterr().out
    assert "output directory already exists" in captured
    assert "Traceback" not in captured
