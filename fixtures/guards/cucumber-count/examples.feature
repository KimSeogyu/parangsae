Feature: Examples count

  Scenario Outline: two tables and a header
    Given <a>
    Then <b>

    Examples:
      | a | b |
      | 1 | 2 |
      | 3 | 4 |

    Examples:
      | a | b |
      | 1 | 2 |
      | 3 | 4 |
      | 5 | 6 |

    Examples:
      | a | b |

  Scenario Outline: no examples
    Given one
    Then two
