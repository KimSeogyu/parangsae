from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import os
import yaml

from packages.core.contracts import ProductFamily

LIVE_ACK_ENV = "PARANGSAE_LIVE_TRADING_ACK"
LIVE_ACK_VALUE = "I_UNDERSTAND_THIS_CAN_LOSE_MONEY"
KILL_SWITCH_ENV = "PARANGSAE_KILL_SWITCH"


@dataclass(frozen=True)
class ProductPolicy:
    product_family: ProductFamily
    venue: str
    short_allowed: bool
    requires_capability: bool
    live_allowed: bool
    requires_tradfi_agreement_state: bool = False
    requires_trading_schedule: bool = False
    fail_closed_on_hidden_constituents: bool = False


@dataclass(frozen=True)
class StrategyPolicy:
    name: str
    enabled_for: tuple[ProductFamily, ...]
    options: dict


@dataclass(frozen=True)
class ProductRisk:
    max_leverage: float
    gross_exposure_cap_pct: float
    net_exposure_cap_pct: float
    max_order_notional_pct: float
    position_caps_pct: dict[str, float]


@dataclass(frozen=True)
class PortfolioRisk:
    daily_loss_stop_pct: float
    rolling_7d_drawdown_reduce_only_pct: float
    hard_drawdown_kill_switch_pct: float
    max_open_orders_total: int
    max_open_orders_per_symbol: int
    max_hourly_turnover_pct: float
    max_daily_turnover_pct: float


@dataclass(frozen=True)
class RiskConfig:
    profile: str
    kill_switch_path: Path
    portfolio: PortfolioRisk
    product_families: dict[ProductFamily, ProductRisk]


def load_yaml(path: str | Path) -> dict:
    with Path(path).open() as f:
        data = yaml.safe_load(f) or {}
    if not isinstance(data, dict):
        raise ValueError(f"YAML root must be a mapping: {path}")
    return data


def load_risk_config(path: str | Path) -> RiskConfig:
    data = load_yaml(path)
    portfolio = PortfolioRisk(**data["portfolio"])
    families = {}
    for key, value in data["product_families"].items():
        family = ProductFamily(key)
        families[family] = ProductRisk(**value)
    cfg = RiskConfig(
        profile=data["profile"],
        kill_switch_path=Path(data["kill_switch_path"]),
        portfolio=portfolio,
        product_families=families,
    )
    validate_balanced_defaults(cfg)
    return cfg


def validate_balanced_defaults(cfg: RiskConfig) -> None:
    if cfg.profile != "balanced":
        raise ValueError("Milestone 1 only supports the balanced risk profile")
    expected_leverage = {
        ProductFamily.SPOT_CRYPTO: 1.0,
        ProductFamily.CRYPTO_PERP: 2.0,
        ProductFamily.TRADFI_PERP: 1.0,
    }
    for family, leverage in expected_leverage.items():
        actual = cfg.product_families[family].max_leverage
        if actual != leverage:
            raise ValueError(f"{family.value}.max_leverage must be {leverage}, got {actual}")
    p = cfg.portfolio
    expected = {
        "daily_loss_stop_pct": 0.02,
        "rolling_7d_drawdown_reduce_only_pct": 0.05,
        "hard_drawdown_kill_switch_pct": 0.10,
        "max_open_orders_total": 10,
        "max_open_orders_per_symbol": 3,
        "max_hourly_turnover_pct": 0.10,
        "max_daily_turnover_pct": 0.50,
    }
    for attr, value in expected.items():
        if getattr(p, attr) != value:
            raise ValueError(f"portfolio.{attr} must be {value}, got {getattr(p, attr)}")


def kill_switch_active(path: str | Path) -> bool:
    env = os.getenv(KILL_SWITCH_ENV, "").strip().lower()
    return env in {"1", "true", "on"} or Path(path).exists()


def load_product_policies(path: str | Path) -> dict[ProductFamily, ProductPolicy]:
    data = load_yaml(path)
    raw = data.get("product_families", {})
    policies: dict[ProductFamily, ProductPolicy] = {}
    for key, value in raw.items():
        family = ProductFamily(key)
        policies[family] = ProductPolicy(
            product_family=family,
            venue=value["venue"],
            short_allowed=bool(value.get("short_allowed", False)),
            requires_capability=bool(value.get("requires_capability", True)),
            live_allowed=bool(value.get("live_allowed", False)),
            requires_tradfi_agreement_state=bool(value.get("requires_tradfi_agreement_state", False)),
            requires_trading_schedule=bool(value.get("requires_trading_schedule", False)),
            fail_closed_on_hidden_constituents=bool(value.get("fail_closed_on_hidden_constituents", False)),
        )
    missing = set(ProductFamily) - set(policies)
    if missing:
        raise ValueError(f"missing product policies: {[m.value for m in missing]}")
    return policies


def load_strategy_policies(path: str | Path) -> dict[str, StrategyPolicy]:
    data = load_yaml(path)
    raw = data.get("strategies", {})
    policies: dict[str, StrategyPolicy] = {}
    for name, value in raw.items():
        enabled_for = tuple(ProductFamily(item) for item in value.get("enabled_for", []))
        policies[name] = StrategyPolicy(name=name, enabled_for=enabled_for, options=dict(value))
    return policies
