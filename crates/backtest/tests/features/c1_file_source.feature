Feature: C1 file admission separates source keys from market values
  Scenario: A missing source key cannot be hidden by a later payload error
    Given an incomplete C1 source file with an invalid price payload
    When the file reader admits the declared C1 day
    Then key coverage is refused before the price payload and no day is admitted
