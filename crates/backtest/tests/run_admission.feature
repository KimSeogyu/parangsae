Feature: Native walk-forward run admission
  Config and pin refusals create no output. A new run claims its output and ledger exclusively.

  Scenario: A wrong expected pin root is rejected
    Given a synthetic pin file with a wrong expected root
    When the native pin file is loaded
    Then the expected pin root is rejected
    And output and ledger are absent

  Scenario: The native batch refuses a wrong expected pin root before creating run paths
    Given a synthetic pin file with a wrong expected root
    When the native batch request runs
    Then the native batch error names pinned_root_expected
    And output and ledger are absent

  Scenario Outline: An invalid expected pin root is rejected
    Given a synthetic pin file with a <kind> expected root
    When the native pin file is loaded
    Then the expected pin root is rejected

    Examples:
      | kind      |
      | short     |
      | nonhex    |
      | null      |

  Scenario Outline: Generic v2 callers retain pin compatibility
    Given a synthetic pin file with a <kind> expected root
    When the native pin file is loaded
    Then the synthetic pin file is accepted

    Examples:
      | kind     |
      | matching |
      | absent   |

  Scenario: An existing empty output directory is refused
    Given an existing empty output directory
    When native run paths are claimed
    Then the run path claim is rejected
    And the existing output remains empty
    And the ledger is absent

  Scenario: An existing empty external ledger is refused
    Given an existing empty external ledger
    When native run paths are claimed
    Then the run path claim is rejected
    And the existing ledger remains empty
    And the claimed output remains empty

  Scenario: A second claim cannot alter the first run's files
    Given a synthetic run has claimed fresh paths and written evidence
    When native run paths are claimed again
    Then the run path claim is rejected
    And the first run's files are unchanged

  Scenario: A valid fresh claim supports the existing trial append behavior
    Given a synthetic pin file with a matching expected root
    When native run paths are claimed
    Then an empty ledger exists under the output directory
    When a synthetic native walk-forward run uses the claimed ledger
    Then both scored segments have one start and one finish

  Scenario Outline: A preflight refusal leaves no run files
    Given a synthetic native request with invalid <kind>
    When the native batch request runs
    Then the native batch request is rejected
    And output and ledger are absent

    Examples:
      | kind   |
      | config |
      | pins   |
