from __future__ import annotations

import json
from pathlib import Path
from typing import Iterable

from parangsae.core.contracts import EventEnvelope, ExecutionEvent, envelope


class EventLog:
    def __init__(self, path: str | Path) -> None:
        self.path = Path(path)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.sequence = self._last_sequence() + 1

    def _last_sequence(self) -> int:
        if not self.path.exists():
            return -1
        last = -1
        with self.path.open() as f:
            for line in f:
                if line.strip():
                    last = json.loads(line)["sequence"]
        return last

    def append(self, event: ExecutionEvent) -> EventEnvelope:
        env = envelope(self.sequence, event)
        with self.path.open("a") as f:
            f.write(env.to_json() + "\n")
        self.sequence += 1
        return env


def read_events(path: str | Path) -> list[dict]:
    p = Path(path)
    if not p.exists():
        return []
    return [json.loads(line) for line in p.read_text().splitlines() if line.strip()]


def replay_positions(events: Iterable[dict]) -> dict[str, float]:
    positions: dict[str, float] = {}
    for env in events:
        event = env["event"]
        if event["event_type"] == "PaperFillRecorded":
            payload = event["payload"]
            qty = float(payload["quantity"])
            if payload["side"] == "SELL":
                qty *= -1
            positions[payload["symbol"]] = positions.get(payload["symbol"], 0.0) + qty
    return {k: v for k, v in sorted(positions.items()) if abs(v) > 1e-12}
