Feature: Rule background count

  Background:
    Given feature background

  Scenario: outside the rule
    Given one
    Then two

  Rule: inner
    Background:
      Given rule background one
      And rule background two

    Scenario: first
      Given only

    Scenario: second
      Given only
