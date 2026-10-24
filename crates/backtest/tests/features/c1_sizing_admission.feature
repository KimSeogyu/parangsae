Feature: C1 pretrade sizing respects known net feasibility and the fixed all-in cap

  Scenario Outline: A known net spot close below minimum is not an entry candidate
    Given a full-capital closed account whose current net spot close is below minimum
    And the selected actual perp entry fee is <fee> bps
    When the actual private C1 sizer evaluates those current terms
    Then the actual result is Refused NoCompatibleSize rather than a sized pair

    Examples:
      | fee |
      | 5   |
      | 6   |

  Scenario Outline: Later wallet profit cannot increase the initial fee-inclusive cap
    Given a richer perp wallet with asymmetric references near its fixed entry cap
    And the selected actual perp entry fee is <fee> bps
    When the actual private C1 sizer evaluates those current terms
    Then the actual sized pair has gross 300000000 and net 299700000 BTC ticks
    And its actual perp entry requirement excluding reserves is <entry> cash ticks within the original cap

    Examples:
      | fee | entry          |
      | 5   | 33764562374265 |
      | 6   | 33767937143118 |
