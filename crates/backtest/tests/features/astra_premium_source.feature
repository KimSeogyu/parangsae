Feature: Archived premium projections have an explicit model clock
  A model-known ratio is separate from its original collector provenance.

  Scenario: A view appears at the complete minute without clock backdating
    Given a synthetic ten-field premium projection
    When the projection is checked under the named Astra minute-end model
    And a view is requested just before the complete minute
    Then no signed ratio is visible yet
    When a view is requested at the complete minute
    Then the signed ratio is visible without rewriting the original clocks

  Scenario: A traded price domain cannot become a premium ratio
    Given a synthetic premium projection with price domain
    When the projection is checked under the named Astra minute-end model
    Then the wrong domain is a typed refusal

  Scenario: Future payloads do not change an earlier source view
    When a future ratio is changed after the earlier decision
    Then only the earlier delivered ratio is observed
