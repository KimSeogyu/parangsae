Feature: H4 economic visibility and actual entry time
  Synthetic inputs expose timing without any market-data replay.

  Scenario Outline: Unpublished economic input cannot size a new entry
    When the <position> funding-slot account is replayed with two unpublished <input> values
    Then the construction has the intended pre-fill position and pending-close history
    And the decision book and requested entry lots before publication are identical

    Examples:
      | position | input |
      | held     | rate  |
      | held     | mark  |

    @control
    Examples:
      | position | input |
      | flat     | rate  |
      | flat     | mark  |

  Scenario Outline: H4 exits from the actual entry fill clock
    When actual H4 waits through <fillers> filler minutes before entry
    Then a real H4 short entry and one time exit are observed
    And the first exit decision is the first hourly decision at or after actual entry plus hold

    Examples:
      | fillers |
      | 0       |
      | 1       |

  @control
  Scenario: A fold boundary close is separately forced
    When actual H4 is flattened by a short scored fold
    Then the fold close is forced before the hold deadline and no order crosses the fold

  @control
  Scenario: A new H4 hold cannot cross the development end
    When actual H4 offers an entry whose hold passes the development end
    Then the entry is refused and its dropped-entry count is visible
