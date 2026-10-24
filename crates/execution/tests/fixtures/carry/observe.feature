Feature: Carry observations sample risk without inventing financial evidence
  This observes the synthetic CarryEngine public API only.
  It does not model historical latency or intraminute liquidation.

  Scenario: An admitted mark trips loss without changing the financial account
    When one matched pair observes an adverse spot mark and then the recovered marks
    Then each admitted observation returns Observed without changing financial evidence
    And the observed loss remains latched after recovery with exactly one new epoch

  Scenario: Observe preserves the original strict legging deadline
    When a dispatched unfilled spot entry is observed at its deadline and one microsecond later
    Then deadline equality is Observed and preserves the complete public snapshot
    And the later observation retains LeggingDeadline and refuses with UnsupportedFact
