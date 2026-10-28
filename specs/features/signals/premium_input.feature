Feature: Premium input has a signed ratio domain
  Source validity, feature availability, and execution liquidity remain separate.

  Scenario Outline: A premium value preserves its sign and zero
    Given a source minute time of 1608249600000000 microseconds
    When a premium input is constructed with close ticks <ticks>
    Then the premium close ticks remain <ticks>
    And the minute time remains 1608249600000000 microseconds

    Examples:
      | ticks |
      | -1    |
      | 0     |
      | 1     |

  Scenario: An off-minute key is not silently rounded
    Given a source minute time of 1608249600000001 microseconds
    When a premium input is constructed with close ticks 0
    Then the off-minute input is refused without rounding

  Scenario: Availability gating does not rewrite archive provenance
    Given an archived premium input wrapped in its unchanged source clocks
    When a causal view is requested one microsecond before availability
    Then no premium value is visible
    When a causal view is requested at availability
    Then the visible premium close ticks are -1
    And the original source timestamps were not rewritten
