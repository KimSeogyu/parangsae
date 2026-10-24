Feature: Backtest engine v0
  A 1m BTCUSDT replay fills at the next open, settles funding on the pre-fill position, and keeps cash in 1e8 ticks.
  Slippage is zero. Initial cash is 1_000 USDT unless the scenario says otherwise.

  Scenario: G1 round trip at 5 bps
    When the G1 round trip is run
    Then there are 2 fills
    And fill 1 is buy at 100 USDT with fee 5000000
    And fill 2 is sell at 110 USDT with fee 5500000
    And the closed trade pnl is 989500000
    And ending equity is 100989500000

  Scenario: G2 fill price is the next bar open
    When the G2 next-open buy is run
    Then there are 1 fills
    And fill 1 is buy at 100 USDT with fee 5000000

  Scenario: G3 a long pays a positive funding rate
    When the G3 funding hold is run
    Then funding amount is -1000000 on position 100000000 at mark 100

  Scenario: G4 a short pays, a short receives, and a flat stamp is zero
    When the G4 short pays case is run
    Then funding amount is -1000000 on position -100000000 at mark 100
    When the G4 short receives case is run
    Then funding amount is 1000000 on position -100000000 at mark 100
    When the G4 flat case is run
    Then funding amount is 0 on position 0 at mark 100

  Scenario: G5 funding uses the pre-fill position
    When the G5 funding and flatten is run
    Then funding amount is -1000000 on position 100000000 at mark 100
    And there are 2 fills
    And fill 2 is sell at 100 USDT with fee 5000000

  Scenario: G6 a one-lot fee rounds away from zero
    When the G6 one-lot buy is run
    Then there are 1 fills
    And fill 1 is buy at 100 USDT with fee 1

  Scenario: G7 a flip splits into two fee legs
    When the G7 flip is run
    Then there are 3 fills
    And the closed trade count is 1
    And the open trade count is 1
    And the closed trade pnl is 489750000

  Scenario: G8 the fee grid equities on the round trip
    When the G8 fee grid is run
    Then fee-grid ending equities are 100995800000,100991600000,100989500000,100987400000

  Scenario: G9 sharpe and max drawdown
    When the G9 buy-and-hold is run
    Then the daily sharpe ticks are 115470053
    And the annual sharpe ticks are 2206052281
    And the max drawdown ticks are 0

  Scenario: G10 a gap, a missing mark, and a missing funding stamp abort
    When the G10 gapped tape is run
    Then the engine error is a gap
    When the G10 missing mark tape is run
    Then the engine error is a missing mark
    When the G10 missing funding tape is run
    Then the engine error is missing funding

  Scenario: G11 ruin freezes equity and stops fills
    When the G11 leveraged long is run
    Then the run is ruined
    And there are 2 fills
    And fill 2 is sell at 50 USDT with fee 2500000
    And equity at bar 3 is -7500000
    And equity at bar 4 equals equity at bar 3

  Scenario: G12 an order on the last bar is dropped
    When the G12 three buys are run
    Then dropped orders are 1
    And there are 2 fills
    And the run is not ruined

  Scenario: T3 a closed hour fills at that hour's next open
    When the T3 hour target is run
    Then there are 1 fills
    And fill 1 is buy at 100 USDT with fee 0

  Scenario: T5 funding is hidden until slot plus 60s and settles before the fill
    When the T5 funding visibility case is run
    Then the 08:00 funding is hidden at 08:00:30
    And the 08:00 funding is visible at 08:01:00
    And funding amount is -1000000 on position 100000000 at mark 100

  Scenario: T7 a listed two-minute mark gap passes and a longer or unlisted gap aborts
    When the T7 two-minute listed gap is run
    Then the run is not ruined
    When the T7 six-minute listed gap is run
    Then the engine error is a missing mark
    When the T7 unlisted gap is run
    Then the engine error is a missing mark

  Scenario: A delayed interior minute cannot reach the hourly signal
    When an interior minute is unavailable until the next hour
    Then the engine error is resample availability

  Scenario: Delayed funding stays outside the causal prefix
    When an earlier funding slot is delayed beyond a later available slot
    Then the delayed funding mutation leaves the signal's funding prefix empty

  Scenario Outline: A satisfied target cancels the pending entry after a filler hour
    When a <kind> target follows an untraded hour
    Then there are 0 fills
    And refused orders are 1
    And dropped orders are 0

    Examples:
      | kind       |
      | stale-flat |
      | zero       |

  Scenario: One unchanged pending order is refused only once across filler hours
    When an unchanged target waits through two untraded hours
    Then there are 1 fills
    And fill 1 is buy at 100 USDT with fee 0
    And refused orders are 1

  Scenario: A decision after the boundary cannot trade at the boundary open
    When a target waits for a decision one millisecond after the boundary
    Then the first fill is the 13:01 open

  Scenario: A target quantity uses its actual execution price
    When the target is half equity and the fill price doubles
    Then there are 1 fills
    And the filled quantity is 5000000 ticks

  Scenario: Compounded drawdown rounds a fractional ratio tick upward
    When the fractional-tick drawdown cash curve is measured
    Then the max drawdown ticks are 28000001

  Scenario: An inactive lookahead probe cannot attest causality
    When the H2 lookahead probe runs with an unreachable deadband
    Then the probe refuses to attest causality without an active decision
