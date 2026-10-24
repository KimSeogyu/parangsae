Feature: C1 causal payloads and the single account actor
  Scenario: Exact causal funding projection
    Given nine causal past funding values of one basis point
    When the actual C1 feature is evaluated
    Then the projections are exactly twenty one and forty two basis points

  Scenario: Missing expected funding cannot select an older value
    Given an older funding value without the expected latest value
    When the actual C1 feature is evaluated
    Then the expected latest value stays unavailable without stale substitution

  Scenario: Exit information does not need complete entry history
    Given a valid nonpositive latest rate and incomplete history
    When the actual C1 feature is evaluated
    Then the latest exit rate remains available without a complete entry feature

  Scenario: Pending generated success does not consume the first complete evaluation
    Given the same key becomes complete after a pending generated evaluation
    When the real generated owner processes pending complete and duplicate evaluations
    Then pending keeps the key and the first complete evaluation consumes it once

  Scenario: Two actual carry cycles preserve a single account
    Given two funded carry cycles with no capital reset
    When the actual C1 actor replays the bounded synthetic source
    Then both cycles retain cash fees funding history and completion clocks

  Scenario: A required mark gap cannot be replaced by an older mark
    Given a held pair whose latest required mark is missing
    When the actual C1 actor replays the bounded synthetic source
    Then the whole replay is technical with its actual financial prefix retained

  Scenario: A known sizing refusal takes precedence over an economic false
    Given a known sizing refusal that overlaps a negative basis
    When the actual actor evaluates both initial arming states
    Then the key is consumed and the prior arm is preserved without a new false

  Scenario: Valid zero count preserves the first valid same key evaluation
    Given an armed flat account and a zero count reference for a new positive key
    When the actual actor receives a valid later frame for the same key
    Then the unavailable frame preserves the arm and the valid frame dispatches once

  Scenario: A flat selected policy fold preserves the existing account
    Given two completed cycles followed by a flat selected policy fold
    When the actual C1 actor replays the bounded synthetic source
    Then both cycles retain cash fees funding history and completion clocks

  Scenario: The actual 336 hour timer triggers the first scheduled exit
    Given a fully observed pair with positive funding through the real 336 hour hold
    When the actor reaches the first scheduled MAX exit after admitted completion
    Then the actual exit uses the unchanged 336 hour timer and preserves all cashflows

  Scenario: A new epoch cannot rearm from an old seen key
    Given a flat fold observed a negative key before a later active epoch
    When the actor sees the old key and then the first post activation key
    Then the old key changes no control or consumption and only the new false arms

  Scenario: An active pair closes at the predeclared fold Terminal
    Given a paired position held until the predeclared fold Terminal
    When the Terminal source has positive count closing minutes
    Then Terminal closes before the original fold end with all costs retained

  Scenario: Zero closing liquidity cannot move the Terminal or extend its deadline
    Given a paired position held until the predeclared fold Terminal
    When the Terminal source has zero count closing minutes
    Then the replay fails at the original fold end with its financial prefix retained

  Scenario: A lawful final cut admitted at E completes the fold
    Given the same predetermined closing spot liquidity at the last pre end minute
    When the canonical pre 2025 spot clock is replayed
    Then the admitted cut at E completes the fold using only pre E physical facts

  Scenario: A pre end spot fact unpublished at E cannot rescue the fold
    Given the same predetermined closing spot liquidity at the last pre end minute
    When the canonical post 2025 spot clock is replayed
    Then the unpublished E plus 1ms receipt leaves a technical failure at E
