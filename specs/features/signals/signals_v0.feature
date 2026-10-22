Feature: Signals v0.1 causal outputs

  Scenario: inclusive decision reads the bar available at that instant
    Given the tsmom toy closes
    When tsmom is evaluated at hour 4
    Then the target ticks are 49385225

  Scenario: one microsecond before availability is not a triggering bar
    Given the tsmom toy closes
    When tsmom is evaluated 1 microsecond before hour 4
    Then the signal error is NoTriggeringBar

  Scenario: funding is visible at funding time plus 60 seconds and not earlier
    Given a funding row at hour 24 with rate 10000
    Then the funding row is visible at hour 24 plus 60000000 microseconds
    And the funding row is not visible at hour 24 plus 59000000 microseconds
    And the funding row is not visible 1 microsecond before hour 24 plus 60000000 microseconds
    Given the funding z toy rows
    When funding z is evaluated at hour 24
    Then there is no signal output

  Scenario: warmup gives no output
    Given the tsmom toy closes
    When tsmom is evaluated at hour 3
    Then there is no signal output

  Scenario: after warmup every bar emits a target
    Given 10 rising hourly closes
    Then tsmom emits a target on hours 4 through 10

  Scenario: a non-finite feature is an error and never a target
    When a rank is computed for NaN
    Then the rank error is NonFinite

  Scenario: a zero close is an invalid feature
    Given one hourly bar with close ticks 0
    When tsmom is evaluated at hour 1
    Then the signal error is InvalidFeature

  Scenario: the target clamps at positive one
    Given the tsmom toy closes
    When tsmom is evaluated at hour 6
    Then the target ticks are 100000000

  Scenario: the target clamps at negative one
    Given hourly closes 40000 40000 40000 20000
    When tsmom is evaluated at hour 4
    Then the target ticks are -100000000

  Scenario: a missing funding slot is flat
    Given the funding gap rows
    When funding z is evaluated at hour 33
    Then the decision is flat FundingSlotMissing

  Scenario: stale funding is flat
    Given the funding z toy rows
    When funding z is evaluated at hour 41
    Then the decision is flat FundingStale

  Scenario: a four hour funding interval is an error
    Given the funding z toy rows with the last interval at 4 hours
    When funding z is evaluated at hour 25
    Then the target ticks are -81649658
    When funding z is evaluated at hour 33
    Then the signal error is FundingIntervalChange

  Scenario: volregime emits a rank and not a target
    Given the volregime toy closes
    When volregime is evaluated at hour 6
    Then the feature rank ticks are 83333333
