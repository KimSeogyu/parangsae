Feature: The shock kernel reads only its declared causal hourly window
  Scenario Outline: Real caller prices reach an active opposite direction
    When the actual shock kernel sees a <shock> USDT shock close and a <confirmation> USDT confirmation close
    Then it emits a causal <side> opportunity for the shock hour two hours before confirmation

    Examples:
      | shock | confirmation | side  |
      | 110   | 104.5        | short |
      | 90    | 94.5         | long  |
      | 110   | 107.25       | short |
      | 110   | 101.75       | short |

  Scenario: A constant baseline cannot silently rearm a controller
    When the same qualifying shock follows a constant 672 return baseline
    Then zero scale is unavailable and is not a known false rearming observation

  Scenario: Past information must discriminate an active decision
    When a past baseline close becomes 200 USDT after the original frame reached an active decision
    Then the real past perturbation changes the decision to known false below the shock threshold

  Scenario: Unseen future information cannot change an active earlier decision
    When future closes become 500 USDT after an earlier active shock decision
    Then the active earlier opportunity is exactly unchanged by future mutation and truncation

  Scenario: The feature cannot invent a later decision clock
    When the qualifying shock is evaluated one millisecond after its declared decision
    Then the actual kernel reports the typed clock error
