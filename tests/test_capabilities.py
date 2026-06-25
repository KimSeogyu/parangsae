from parangsae.core.capabilities import fixture_capability
from parangsae.core.contracts import ProductFamily


def test_tradfi_perp_fixture_fails_closed_until_capability_known():
    cap = fixture_capability("AAPLUSDT", ProductFamily.TRADFI_PERP)
    eligible, reason = cap.live_eligible()
    assert not eligible
    assert reason in {"tradfi_schedule_unknown", "tradfi_agreement_not_confirmed", "tradfi_constituent_prices_hidden"}


def test_hidden_constituent_prices_reject_when_other_gates_known():
    cap = fixture_capability("AAPLUSDT", ProductFamily.TRADFI_PERP)
    object.__setattr__(cap, "schedule_known", True)
    object.__setattr__(cap, "agreement_known", True)
    object.__setattr__(cap, "agreement_signed", True)
    eligible, reason = cap.live_eligible()
    assert not eligible
    assert reason == "tradfi_constituent_prices_hidden"
