Feature: Paper risk authority and atomic settlement
  Scenario: Opposing orders retain independent reservations
    Given a flat declared paper portfolio
    When one BTC buy and one BTC sell are approved
    Then a further buy is refused by the position limit

  Scenario: A halt invalidates an unsent approval
    Given a flat declared paper portfolio
    When an approved buy is followed by a kill latch
    Then its dispatch is refused without a fill

  Scenario: A late terminal mismatch cannot partially apply a receipt batch
    Given a flat declared paper portfolio
    When a dispatched buy receives a priced half fill and an inconsistent terminal cut
    Then position remains flat and its reservation remains held

  Scenario: A moving-mark breach permits a strict protective close
    Given a declared one BTC long paper portfolio
    When a higher causal mark breaches its notional bound
    Then a fresh reduce-only half close is approved while risk remains halted

  Scenario: A refused identity cannot be rebound to the opposite side
    Given a flat declared paper portfolio
    When a held buy causes another buy to be refused
    Then changing the refused order to a sell halts without a fill
