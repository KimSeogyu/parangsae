Feature: FLAT folds retain their scored time in stitched exposure
  A FLAT fold contributes its full scored duration and no position, costs, or trades.

  Scenario Outline: An invested fold and an equal FLAT fold have half exposure
    When one invested fold and one equal FLAT fold are stitched
    Then the <replays> have 50 percent exposure across 4 days
    And the <replays> retain only the invested fold's activity

    Examples:
      | replays                |
      | selection cost replays |
      | other fee replays      |

  Scenario Outline: An all-FLAT path keeps time but has no activity
    When two equal FLAT folds are stitched
    Then the <replays> have 0 percent exposure across 4 days
    And the <replays> have no activity

    Examples:
      | replays                |
      | selection cost replays |
      | other fee replays      |
