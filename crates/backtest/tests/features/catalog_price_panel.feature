Feature: The eight price and panel catalogue families expose real causal kernels
  Scenario Outline: Distinct closed-price hypotheses have observable directions
    Given a synthetic catalogue price fixture for <family>
    When the catalogue price kernel evaluates at the third close
    Then the catalogue price direction is <direction>
    Examples:
      | family              | direction |
      | momentum            | long      |
      | mean_reversion      | short     |
      | trend_following     | long      |
      | short_term_reversal | short     |
      | breakout            | long      |

  Scenario: A future extreme price cannot change the admitted signal
    Given a synthetic catalogue price fixture for momentum
    And a later extreme bar is appended after the decision
    When the catalogue price kernel evaluates at the third close
    Then the catalogue price direction is long

  Scenario: Threshold equality is neutral
    Given a synthetic catalogue price fixture for momentum
    And the candidate price is exactly at the one percent threshold
    When the catalogue price kernel evaluates at the third close
    Then the catalogue price direction is neutral

  Scenario: Missing history never masquerades as neutral
    Given a synthetic catalogue price fixture for momentum
    And the candidate is absent
    When the catalogue price kernel evaluates at the third close
    Then the catalogue result is typed unavailable history, not a flat signal

  Scenario: Invalid source clocks are refused
    Given a synthetic catalogue price fixture for momentum
    And the first bar claims availability before its canonical close
    When the catalogue price kernel evaluates at the third close
    Then the catalogue price input is refused with Clock

  Scenario: Pairs trading fades only the supplied prior residual model
    When the supplied pair residual rises from zero to twenty price ticks
    Then the pair assessment shorts left and buys right with the frozen unit hedge

  Scenario Outline: Cross-sectional and factor portfolios use different supplied scores
    When the supplied panel is evaluated by <family>
    Then the exact panel is long <long> and short <short>
    Examples:
      | family                | long | short |
      | cross_sectional_alpha | AAA  | CCC   |
      | factor_investing      | CCC  | AAA   |

  Scenario: Causality is rechecked at panel use
    When a later causal panel view is reused for an earlier decision
    Then the panel is refused with Future
