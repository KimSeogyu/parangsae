Feature: C1 source observations preserve clock meaning
  Scenario: A microsecond spot bar becomes evidence at its modeled delivery
    Given a closed 2025 spot source bar with its original archive receipt
    When the C1 reader checks before and at its modeled delivery
    Then the source clocks stay unchanged and only modeled evidence is admitted

  Scenario: Missing current risk data cannot become a carried mark
    Given an older risk mark but no latest expected risk mark
    When a required C1 mark context is requested
    Then the latest key gap is returned without mark carry
