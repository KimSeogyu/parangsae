Feature: Shared trial ledger retains typed C1 role annotations
  Scenario: Multiple train units retain a single charged identity per candidate
    Given five C1 roles with two train units per candidate
    When the shared native ledger writes finishes and aborts
    Then each terminal keeps its start role and only five charges are counted
