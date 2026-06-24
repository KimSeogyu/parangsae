from __future__ import annotations

import io
import os
import zipfile
from argparse import Namespace
from pathlib import Path


from apps.cli import quantctl
from packages.marketdata.binance_vision import CANONICAL_MONTHS, canonical_urls


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


class FakeResponse:
    def __init__(self, content: bytes):
        self.content = content
        self.status_code = 200
        self.headers = {"ETag": "fixture", "Last-Modified": "fixture"}

    def raise_for_status(self):
        return None


def args(tmp_path: Path) -> Namespace:
    return Namespace(
        symbol="BTCUSDT",
        interval="1h",
        months=",".join(CANONICAL_MONTHS),
        cache_dir=str(tmp_path / "cache"),
        reports_root=str(tmp_path / "reports"),
        output_dir=None,
        overwrite=False,
    )


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
    source = Path("apps/cli/quantctl.py").read_text()
    real_run_source = source[source.index("def cmd_real_run") : source.index("def cmd_kill_switch")]
    assert "ccxt" not in real_run_source
    assert "create_order" not in real_run_source
    assert "BINANCE_API_KEY" not in real_run_source
    assert "BINANCE_API_SECRET" not in real_run_source


def test_real_run_rejects_noncanonical_window_with_failure_artifact(tmp_path):
    a = args(tmp_path)
    a.months = "2024-02"
    assert quantctl.cmd_real_run(a) == 2
    report_dirs = list((tmp_path / "reports").iterdir())
    assert len(report_dirs) == 1
    assert (report_dirs[0] / "failure.json").exists()
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
