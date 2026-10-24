Feature: A perpetual close preflights its executed upper price without another slippage charge
  This checks synthetic accounting and dispatch, not historical exchange execution.

  Scenario Outline: A funded upper bound permits a close and actual fills retain configured slippage
    When a held pair with <slip> bps perpetual slippage dispatches a close capped at 101 USDT
    Then the funded close dispatches without posting cash, fees, PnL or a priced fill
    When the raw 100 USDT buyback and spot exit complete with priced cuts
    Then the executed price, fees and final wallet PnL match one slippage application

    Examples:
      | slip |
      | 0    |
      | 2    |

  Scenario: An actual fill above the executed upper remains refused
    When a held pair with 2 bps perpetual slippage dispatches a close capped at 101 USDT
    Then the funded close dispatches without posting cash, fees, PnL or a priced fill
    When a raw 101 USDT buyback is delivered against the 101 USDT executed cap
    Then PriceEnvelope retains the unapplied fact without changing financial evidence

  Scenario: The worst permitted close must still fit actual isolated cash
    When the fee-inclusive zero-cash boundary and one tick less preflight a 300 USDT upper
    Then only the funded boundary dispatches and neither preflight posts or releases reserves
