import json
import subprocess
import sys

from parangsae.core.events import read_events, replay_positions


def test_paper_run_and_replay(tmp_path):
    log = tmp_path / "events.jsonl"
    subprocess.run([sys.executable, "-m", "parangsae.apps.cli.quantctl", "paper-run", "--fixture", "--ticks", "3", "--event-log", str(log)], check=True)
    events = read_events(log)
    assert events
    positions = replay_positions(events)
    assert positions


def test_backtest_report_no_profit_guarantee():
    result = subprocess.run([sys.executable, "-m", "parangsae.apps.cli.quantctl", "backtest", "--fixture"], check=True, capture_output=True, text=True)
    data = json.loads(result.stdout)
    assert data["no_profit_guarantee"] is True
