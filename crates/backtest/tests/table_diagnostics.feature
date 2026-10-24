Feature: Report diagnostics preserve undefined values and exact gate boundaries

  Scenario: An all-FLAT fee grid has no estimated fee slope or breakeven
    When an all-FLAT fee grid is written to the report CSV
    Then the reported annual Sharpe is undefined
    And the reported fee sensitivity and breakeven fee are undefined
