Feature: The causal range feature distinguishes a known condition from missing input
  Scenario Outline: Prior-only quarter ATR and strict inside close determine direction
    Given a complete synthetic range window has 24 prior ranges from 976 to 1024
    When the candidate has high <high> low <low> and close <close>
    Then the pure range result is <condition>

    Examples:
      | high | low | close | condition   |
      | 1036 | 980 | 1000  | known_false |
      | 1037 | 980 | 1000  | short       |
      | 1020 | 964 | 1000  | known_false |
      | 1020 | 963 | 1000  | long        |
      | 1037 | 963 | 1000  | known_false |
      | 1024 | 976 | 1000  | known_false |
      | 1037 | 975 | 1000  | short       |
      | 1037 | 976 | 976   | known_false |
      | 1037 | 976 | 1024  | known_false |

  Scenario Outline: Missing or undefined input never becomes an observed false
    Given a complete synthetic range window has 24 prior ranges from 976 to 1024
    When the range window has <case>
    Then the result is unavailable for <reason> and is not observed false

    Examples:
      | case             | reason           |
      | short_history    | window_incomplete |
      | missing_candidate | window_incomplete |
      | known_gap        | window_incomplete |
      | zero_atr         | zero_atr          |
      | degenerate_range | degenerate_range  |

  Scenario Outline: Malformed metadata is refused without manufacturing a false
    Given a complete synthetic range window has 24 prior ranges from 976 to 1024
    When the otherwise active range input has <case> metadata
    Then the pure API returns a typed refusal instead of a trading condition

    Examples:
      | case               |
      | duplicate          |
      | wrong_interval     |
      | wrong_availability |
      | wrong_decision     |

  Scenario: Future perturbation is inert while an admitted past dependency is active
    Given a complete synthetic range window has 24 prior ranges from 976 to 1024
    When unavailable future OHLC changes while the admitted seed remains fixed
    Then the same active short result survives but changing the admitted seed changes the result
