Feature: H4 output preserves checked cash and execution provenance
  Scenario Outline: The private report exposes the frozen diagnostic definitions
    Given the synthetic H4 report case <case>
    When the H4 output files are written
    Then the H4 diagnostic values and existing CSV columns match <case>

    Examples:
      | case              |
      | signed_cash       |
      | undefined_gross   |
      | forced_terminal   |
      | forced_ruin       |
      | concentration     |
      | lost_provenance   |
      | native_adapter    |
