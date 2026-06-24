from packages.core.contracts import (
    ExecutionEvent,
    ExecutionEventType,
    ProductFamily,
    SignalFrame,
    canonical_json,
    envelope,
)


def test_contract_serialization_is_deterministic():
    signal = SignalFrame("trend-risk", ProductFamily.SPOT_CRYPTO, "BTCUSDT", "2026-01-01T00:00:00Z", 0.1, "x")
    left = canonical_json(signal.__dict__)
    right = canonical_json(signal.__dict__)
    assert left == right
    assert "spot_crypto" in left


def test_event_envelope_schema_version():
    env = envelope(0, ExecutionEvent(ExecutionEventType.SIGNAL, {"symbol": "BTCUSDT"}), event_id="evt-test")
    data = env.to_dict()
    assert data["schema_version"] == 1
    assert env.to_json() == env.to_json()
