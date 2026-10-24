Feature: The C1 unit driver preserves calendar capital and durable role accounting
  Scenario: An explicitly flat synthetic unit still has one full capital day
    When a real isolated cash only C1 unit is assembled with an explicit flat policy
    Then its full capital day is retained and the shared ledger has one matched role bound finish

  Scenario: A ledger IO interruption retains previously completed unit evidence
    When ledger append becomes unavailable after one completed synthetic unit
    Then prior full unit evidence is written and the next unit is explicitly not executed

  Scenario: The actual seven-file report preserves partial evidence
    When the actual native writer emits an incomplete synthetic C1 report
    Then all seven declared artifacts preserve the completed unit without a complete study verdict
