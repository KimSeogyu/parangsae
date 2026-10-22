Feature: Kline time integrity
  A kline is usable only when its clock matches the source declaration and the bar grid.
  Unit conversion never guesses. close_time is open_time plus the interval minus one declared source tick.
  A millisecond source subtracts 1ms. A microsecond source subtracts 1µs. The other close fails.

  Scenario: a millisecond futures open becomes microseconds and a one-minute close
    Given the raw timestamp 1609459200000
    When it is read as milliseconds
    Then the normalized timestamp is 1609459200000000 microseconds
    And the 1m close time is 1609459259999000 microseconds

  Scenario: a microsecond spot timestamp is rejected when milliseconds were declared
    Given the raw timestamp 1792454400000000
    When it is read as milliseconds
    Then the read fails as a unit mismatch

  Scenario: a millisecond futures timestamp is rejected when microseconds were declared
    Given the raw timestamp 1609459200000
    When it is read as microseconds
    Then the read fails as a unit mismatch

  Scenario: a timestamp whose width is neither 13 nor 16 digits is rejected
    Given the raw timestamp 16094592000000
    When it is read as milliseconds
    Then the read fails as a unit mismatch

  Scenario: close time must be open time plus the interval minus one millisecond
    Given a 1m span opening at 1609459200000000 microseconds
    And closing at 1609459259999000 microseconds
    And the declared close tick is milliseconds
    Then the span matches the interval

  Scenario: a close time equal to the next open is rejected
    Given a 1m span opening at 1609459200000000 microseconds
    And closing at 1609459260000000 microseconds
    And the declared close tick is milliseconds
    Then the span does not match the interval

  Scenario: a declared microsecond open closes one microsecond before the next bar
    Given the raw timestamp 1735689600000000
    When it is read as microseconds
    Then the normalized timestamp is 1735689600000000 microseconds
    And the 1m close time is 1735689659999999 microseconds

  Scenario: a declared microsecond close is one source tick before the next open
    Given a 1m span opening at 1735689600000000 microseconds
    And closing at 1735689659999999 microseconds
    And the declared close tick is microseconds
    Then the span matches the interval

  Scenario: a microsecond close fails when the declared tick is one millisecond
    Given a 1m span opening at 1735689600000000 microseconds
    And closing at 1735689659999999 microseconds
    And the declared close tick is milliseconds
    Then the span does not match the interval

  Scenario: a millisecond close fails when the declared tick is one microsecond
    Given a 1m span opening at 1735689600000000 microseconds
    And closing at 1735689659999000 microseconds
    And the declared close tick is microseconds
    Then the span does not match the interval

  Scenario: a continuous one-minute grid is accepted
    Given 1m open times in microseconds
      | open_us          |
      | 1609459200000000 |
      | 1609459260000000 |
      | 1609459320000000 |
    Then the open-time grid is continuous

  Scenario: a missing one-minute bar is a gap
    Given 1m open times in microseconds
      | open_us          |
      | 1609459200000000 |
      | 1609459320000000 |
    Then the open-time grid has a gap
