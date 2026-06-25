from parangsae.core.capabilities import fixture_capability
from parangsae.core.config import LIVE_ACK_ENV, LIVE_ACK_VALUE, load_product_policies, load_risk_config
from parangsae.core.contracts import OrderIntent, OrderSide, OrderType, ProductFamily, Venue
from parangsae.core.risk import LiveTradingGuard, RuntimeLiveContext


def _order(family=ProductFamily.SPOT_CRYPTO, leverage=1.0, notional=0.01):
    venue = Venue.BINANCE_SPOT if family is ProductFamily.SPOT_CRYPTO else Venue.BINANCE_USDS_FUTURES
    return OrderIntent("x", family, venue, "BTCUSDT", OrderSide.BUY, OrderType.LIMIT, 1.0, leverage=leverage, notional_pct=notional)


def _risk():
    return load_risk_config("configs/risk.balanced.yaml")


def test_balanced_defaults_exact():
    cfg = _risk()
    assert cfg.product_families[ProductFamily.SPOT_CRYPTO].max_leverage == 1.0
    assert cfg.product_families[ProductFamily.CRYPTO_PERP].max_leverage == 2.0
    assert cfg.product_families[ProductFamily.TRADFI_PERP].max_leverage == 1.0
    assert cfg.portfolio.daily_loss_stop_pct == 0.02
    assert cfg.portfolio.rolling_7d_drawdown_reduce_only_pct == 0.05
    assert cfg.portfolio.hard_drawdown_kill_switch_pct == 0.10
    assert cfg.portfolio.max_open_orders_total == 10
    assert cfg.portfolio.max_open_orders_per_symbol == 3


def test_high_leverage_rejected_before_live_checks():
    guard = LiveTradingGuard(_risk(), load_product_policies("configs/products.yaml"))
    decision = guard.evaluate(_order(leverage=2.0), fixture_capability("BTCUSDT", ProductFamily.SPOT_CRYPTO), RuntimeLiveContext(mode="live", live_enabled=True))
    assert decision.reason == "leverage_above_balanced_default"


def test_live_ack_exact_near_miss_fails(monkeypatch):
    guard = LiveTradingGuard(_risk(), load_product_policies("configs/products.yaml"))
    monkeypatch.setenv(LIVE_ACK_ENV, LIVE_ACK_VALUE.lower())
    monkeypatch.setenv("BINANCE_API_KEY", "x")
    monkeypatch.setenv("BINANCE_API_SECRET", "x")
    decision = guard.evaluate(
        _order(),
        fixture_capability("BTCUSDT", ProductFamily.SPOT_CRYPTO),
        RuntimeLiveContext(mode="live", live_enabled=True, live_allowed_product_families=("spot_crypto",)),
    )
    assert decision.reason == "live_ack_missing_or_invalid"


def test_live_guard_can_approve_when_all_gates_pass(monkeypatch, tmp_path):
    cfg = _risk()
    object.__setattr__(cfg, "kill_switch_path", tmp_path / "KILL_SWITCH")
    monkeypatch.setenv(LIVE_ACK_ENV, LIVE_ACK_VALUE)
    monkeypatch.setenv("BINANCE_API_KEY", "x")
    monkeypatch.setenv("BINANCE_API_SECRET", "x")
    decision = LiveTradingGuard(cfg, load_product_policies("configs/products.yaml")).evaluate(
        _order(),
        fixture_capability("BTCUSDT", ProductFamily.SPOT_CRYPTO),
        RuntimeLiveContext(mode="live", live_enabled=True, live_allowed_product_families=("spot_crypto",)),
    )
    assert decision.status == "APPROVED"


def test_kill_switch_blocks(monkeypatch, tmp_path):
    cfg = _risk()
    kill = tmp_path / "KILL_SWITCH"
    kill.write_text("on")
    object.__setattr__(cfg, "kill_switch_path", kill)
    monkeypatch.setenv(LIVE_ACK_ENV, LIVE_ACK_VALUE)
    monkeypatch.setenv("BINANCE_API_KEY", "x")
    monkeypatch.setenv("BINANCE_API_SECRET", "x")
    decision = LiveTradingGuard(cfg, load_product_policies("configs/products.yaml")).evaluate(
        _order(),
        fixture_capability("BTCUSDT", ProductFamily.SPOT_CRYPTO),
        RuntimeLiveContext(mode="live", live_enabled=True, live_allowed_product_families=("spot_crypto",)),
    )
    assert decision.reason == "kill_switch_active"


def test_tradfi_live_disabled_by_product_config_even_when_runtime_allows(monkeypatch, tmp_path):
    from parangsae.core.config import load_product_policies
    from parangsae.core.contracts import ProductCapability

    cfg = _risk()
    object.__setattr__(cfg, "kill_switch_path", tmp_path / "KILL_SWITCH")
    monkeypatch.setenv(LIVE_ACK_ENV, LIVE_ACK_VALUE)
    monkeypatch.setenv("BINANCE_API_KEY", "x")
    monkeypatch.setenv("BINANCE_API_SECRET", "x")
    cap = ProductCapability(
        ProductFamily.TRADFI_PERP,
        "AAPLUSDT",
        Venue.BINANCE_USDS_FUTURES,
        tradable=True,
        short_allowed=True,
        schedule_known=True,
        agreement_known=True,
        agreement_signed=True,
        constituent_prices_hidden=False,
    )
    decision = LiveTradingGuard(cfg, load_product_policies("configs/products.yaml")).evaluate(
        _order(ProductFamily.TRADFI_PERP, leverage=1.0, notional=0.01),
        cap,
        RuntimeLiveContext(mode="live", live_enabled=True, live_allowed_product_families=("tradfi_perp",)),
    )
    assert decision.reason == "product_family_live_disabled_by_config"


def test_missing_product_policy_fails_closed_for_live_intent(monkeypatch, tmp_path):
    cfg = _risk()
    object.__setattr__(cfg, "kill_switch_path", tmp_path / "KILL_SWITCH")
    monkeypatch.setenv(LIVE_ACK_ENV, LIVE_ACK_VALUE)
    monkeypatch.setenv("BINANCE_API_KEY", "x")
    monkeypatch.setenv("BINANCE_API_SECRET", "x")
    from parangsae.core.contracts import ProductCapability

    cap = ProductCapability(
        ProductFamily.TRADFI_PERP,
        "AAPLUSDT",
        Venue.BINANCE_USDS_FUTURES,
        tradable=True,
        short_allowed=True,
        schedule_known=True,
        agreement_known=True,
        agreement_signed=True,
        constituent_prices_hidden=False,
    )
    decision = LiveTradingGuard(cfg).evaluate(
        _order(ProductFamily.TRADFI_PERP, leverage=1.0, notional=0.01),
        cap,
        RuntimeLiveContext(mode="live", live_enabled=True, live_allowed_product_families=("tradfi_perp",)),
    )
    assert decision.reason == "product_policy_missing"
