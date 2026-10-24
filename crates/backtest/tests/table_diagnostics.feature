Feature: Report diagnostics preserve undefined values and exact gate boundaries

  Scenario: An all-FLAT fee grid has no estimated fee slope or breakeven
    When an all-FLAT fee grid is written to the report CSV
    Then the reported annual Sharpe is undefined
    And the reported fee sensitivity and breakeven fee are undefined

  Scenario Outline: H2 compares its blend against the exact member median
    When H2 is reported with middle members 200000001 and <upper> and blend 200000001 ticks
    Then the reported H2 gate 2-prime is <passes>
    And the displayed blend Sharpe and member median are both 2.00000001

    Examples:
      | upper     | passes |
      | 200000001 | true   |
      | 200000002 | false  |
