import pytest

from packages.core.config import load_product_policies, load_strategy_policies
from packages.core.contracts import ProductFamily


def test_product_policies_are_runtime_owned():
    policies = load_product_policies("configs/products.yaml")
    assert policies[ProductFamily.TRADFI_PERP].live_allowed is False
    assert policies[ProductFamily.SPOT_CRYPTO].live_allowed is True


def test_strategy_policies_gate_family_use():
    policies = load_strategy_policies("configs/strategies.yaml")
    assert ProductFamily.TRADFI_PERP in policies["trend-risk"].enabled_for
    assert ProductFamily.SPOT_CRYPTO not in policies["crypto-residual-ridge"].enabled_for


def test_cli_strategy_gate_rejects_wrong_family():
    from apps.cli.quantctl import _signals

    with pytest.raises(ValueError):
        _signals("crypto-residual-ridge", ProductFamily.SPOT_CRYPTO)
