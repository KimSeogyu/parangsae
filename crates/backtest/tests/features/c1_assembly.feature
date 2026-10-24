Feature: The C1 unit driver preserves calendar capital and durable role accounting
  Scenario: An explicitly flat synthetic unit still has one full capital day
    When a real isolated cash only C1 unit is assembled with an explicit flat policy
    Then its full capital day is retained and the shared ledger has one matched role bound finish
