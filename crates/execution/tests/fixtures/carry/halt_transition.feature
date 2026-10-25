Feature: Actual carry close transitions retain the first halt cause and assessment
  Scenario: A within limit close does not manufacture a halt
    When a paired carry closes at 1001 with a declared inside notional boundary
    Then all four actual fills and complete cuts retain exact fee inclusive flat wallets
    And no halt or first evidence precedes the later explicit manual halt

  Scenario: Equality at both declared limits remains healthy
    When a paired carry closes at 1001 with a declared equal notional boundary
    Then all four actual fills and complete cuts retain exact fee inclusive flat wallets
    And no halt or first evidence precedes the later explicit manual halt

  Scenario: A notional only breach remains explainable through legal liquidation
    When a paired carry closes at 1002 with a declared over notional boundary
    Then all four actual fills and complete cuts retain exact fee inclusive flat wallets
    And the first Fill assessment distinguishes notional excess from quantity loss and margin
    And the original evidence survives legal liquidation later marks and a second halt cause
