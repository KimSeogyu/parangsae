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
    When the G9 levels are scored
    Then the daily sharpe is 1.1547005383792515
    And the max drawdown ticks are 10000000

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
