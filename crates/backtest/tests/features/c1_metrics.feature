Feature: C1 metrics report completed continuous accounts
  Scenario: Forced unwind results stay in full-capital wealth
    Given a completed C1 metrics fixture named continuous
    When the metrics result is serialized
    Then the reported calendar_days is 3
    And the reported total_return_ticks is 8900000
    And the reported daily_close_max_drawdown_ticks is 10000000
    And the reported normal_round_trips is 0
    And the reported hedge_unwind_pairs is 3

  Scenario: Cash keeps every calendar day
    Given a completed C1 metrics fixture named flat
    When the metrics result is serialized
    Then the reported calendar_days is 4
    And the reported cash_only_days is 4
    And the reported total_return_ticks is 0
    And the annual Sharpe is undefined

  Scenario: A missing day cannot become a zero return
    Given a completed C1 metrics fixture named missing
    When the metrics result is serialized
    Then the metrics refusal is calendar

  Scenario: A technical prefix cannot become a strategy result
    Given a completed C1 metrics fixture named technical
    When the metrics result is serialized
    Then the metrics refusal is technical
