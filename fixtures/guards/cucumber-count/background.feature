Feature: Background count

  Background:
    Given a background step
    And another background step

  Scenario: three steps
    Given one
    When two
    Then three

  Scenario: one step
    Given only
