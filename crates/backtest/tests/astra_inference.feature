Feature: Exact Astra cash diagnostics through the actual CLI
  Scenario: Synthetic numbers remain exact and never become trading authority
    Given a recorded Python interpreter at version 3.11 or newer
    And the stdlib synthetic arithmetic and input-boundary cases pass
    And a complete synthetic cash fixture and fake critical value tagged NOT RUN
    When the actual inference CLI writes the requested diagnostic
    Then exact points and block variances match the independent synthetic oracle
    When the CLI is asked to reuse that output path
    Then it refuses and preserves the existing artifact and both inputs
