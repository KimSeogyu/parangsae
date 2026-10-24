Feature: Terminal execution is fixed before future liquidity is observed
  All inputs are synthetic. Terminal count is the only mutated field.

  Scenario Outline: Future terminal count cannot change an earlier fill
    When each one-to-five-minute terminal count tail is changed for the <scope> boundary
    Then every accepted fill prefix before the first changed count is unchanged
    Examples:
      | scope       |
      | fold        |
      | development |

  Scenario Outline: An unfillable fixed last scheduled open refuses the whole run
    When the fixed last scheduled <scope> open has count zero
    Then no accepted report substitutes an earlier terminal close
    Examples:
      | scope       |
      | fold        |
      | development |

  @control
  Scenario Outline: The scheduled last open remains a costed positive control
    When the fixed last scheduled <scope> open is tradable
    Then the actual terminal close occurs exactly at that scheduled open once
    Examples:
      | scope       |
      | fold        |
      | development |

  Scenario Outline: Terminal cancels pending entry risk while actually flat
    When a pending flat entry reaches the <scope> Terminal with final count <count>
    Then the entry is cancelled without a fill or fee and retains one refusal
    Examples:
      | scope       | count |
      | fold        | 1     |
      | fold        | 0     |
      | development | 1     |
      | development | 0     |

  Scenario: A predeclared Terminal close wins a co-triggered admitted Ruin
    When the admitted mark shock co-triggers Ruin at a tradable Terminal open
    Then Terminal closes once at that open without a backdated forced fill

  Scenario: Hidden control divergence precedes even a predeclared Terminal action
    When unknown funding makes economic and admitted solvency disagree at Terminal
    Then the whole trial refuses before accepting a terminal action
