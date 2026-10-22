Feature: BTCUSDT instrument rules
  A constant is looked up by symbol, field, and a UTC instant.
  Inside an unresolved bracket the conservative policy picks the higher fee.
  Reject leaves that bracket unresolved. Tick size is its own field.

  Scenario: the tick size changes at 03:30 UTC on 2022-02-15
    When BTCUSDT tick size is read at 2022-02-15 03:29:00 UTC with policy conservative
    Then the decimal ticks are 1000000
    When BTCUSDT tick size is read at 2022-02-15 03:30:00 UTC with policy conservative
    Then the decimal ticks are 10000000

  Scenario: conservative VIP0 taker fee inside the 2023 bracket is the higher fee
    When BTCUSDT taker fee is read at 2023-09-01 00:00:00 UTC with policy conservative
    Then the decimal ticks are 50000

  Scenario: reject leaves the taker bracket unresolved
    When BTCUSDT taker fee is read at 2023-09-01 00:00:00 UTC with policy reject
    Then the lookup is unresolved
