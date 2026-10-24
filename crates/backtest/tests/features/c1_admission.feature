Feature: C1 metadata admission preserves a fresh one-shot boundary
  Scenario: A wrong role identity cannot require a market payload
    Given a C1 metadata fixture without market payloads
    When the proposed candidate identity is changed
    Then C1 refuses the identity before requiring a data file or output

  Scenario: A wrong portable pin cannot claim output
    Given a C1 metadata fixture without market payloads
    When the expected portable pin is changed
    Then C1 refuses the pin without creating output or ledger

  Scenario: A complete synthetic snapshot claims a fresh empty ledger once
    Given an independent readonly synthetic C1 object tree
    When C1 admits the synthetic object tree once
    Then the new ledger is empty and a second admission is refused

  Scenario: A recomputed caller identity cannot choose a source policy
    Given a C1 metadata fixture without market payloads
    When the caller changes the source policy and recomputes every identity
    Then the actual Data profile rejects it before payloads or output

  Scenario: A recomputed caller identity cannot change the actual capital model
    Given a C1 metadata fixture without market payloads
    When the caller changes capital and recomputes every identity
    Then the actual runtime contract rejects it before payloads or output
