Feature: C1 risk exits and selected cost paths retain the full synthetic account
  These cases use bounded invented data and do not establish historical liquidity.

  Scenario: An unpaid settlement does not postpone a known risk reduction
    When a retained settlement is unpaid when a known loss stops the paired actor
    Then the actual close order precedes the late payment and preserves its captured quantity and deadline

  Scenario Outline: Each selected cost path is exercised through the complete actor
    When the affordable two-cycle actor runs the <scenario> cost path
    Then every executed leg fee payment and final wallet matches the independent cost oracle

    Examples:
      | scenario   |
      | headline   |
      | higher-fee |
      | stress     |

  Scenario: A future liquid bar cannot repair an expired one-leg pair
    When the paid spot leg has no hedge liquidity until after the original 600 second deadline
    Then the replay stays technical with paid inventory and its undrained order instead of using the late valid bar
