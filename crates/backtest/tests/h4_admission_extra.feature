Feature: H4 full dependency admission and causal liquidation
  These fixtures use generated bars and rates, never market payloads.

  Scenario Outline: Every actual H4 hold uses its filled entry
    When actual H4 holds 168 hours after <fillers> extra filler minutes
    Then its first exit decision is the hourly ceiling of actual fill plus 168 hours
    Examples:
      | fillers |
      | 0       |
      | 1       |

  Scenario: A later known funding row can precede an earlier delayed row
    When two signed delayed earlier rates accompany a later rate available at the first probe
    Then the first probe admits only the later cash while the signal prefix remains blocked
    And exact availability at the second probe admits both signed earlier cash values

  Scenario Outline: Funding cash waits for its full original dependency set
    When the funding money probe varies a <dependency> dependency
    Then decision equity and active requested lots before full publication are equal
    And exact publication controls reveal the original economic difference
    Examples:
      | dependency          |
      | delayed mark        |
      | delayed rate        |

  Scenario: Occupied control requires its original carried mark
    When an occupied control has an unpublished original carried mark
    Then both hidden mark values refuse at the control clock without an older mark fallback

  Scenario: A published original carried mark is an active positive control
    When the same original carried mark is published at the occupied control
    Then both published carried controls complete the active cash and actual fill observations

  Scenario Outline: Unpublished economic overflow is a technical failure
    When an unpublished <input> overflows economic funding with representable visible money
    Then checked technical failure returns no accepted report or later decision
    And the corresponding nonoverflow control completes an active reentry
    Examples:
      | input |
      | rate  |
      | mark  |

  Scenario: Known arithmetic overflow cannot partially mutate the Sim ledger
    When explicitly known extreme funding arithmetic is applied to a settled synthetic ledger
    Then the typed overflow preserves the whole ledger and ordinary signed funding still works

  Scenario: A published ruin trigger liquidates later and still pays funding
    When an admitted short mark shock waits through filler minutes and a funding settlement
    Then its Ruin close is costed strictly after the original mark publication
    And funding continues on the held position until actual flat before freezing

  Scenario: Ruin without a future eligible close is a technical failure
    When no tradable close remains after the admitted short mark shock
    Then unfilled liquidation fails with the actual trigger and no invented report
