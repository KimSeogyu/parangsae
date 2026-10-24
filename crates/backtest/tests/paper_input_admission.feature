Feature: Native paper input admission before durable observation
  A journal accepts only commands that its own decoder can restore.
  Schema-valid engine refusals remain durable observations.

  Scenario Outline: Native values outside the wire domain are refused without mutation
    When a native command has invalid <field>
    Then the invalid native command is refused before journaling
    And the journal bytes, command count and engine snapshot are unchanged
    And the journal resumes and a valid fill can still settle once

    Examples:
      | field                  |
      | fill id                |
      | fill quantity          |
      | fill price             |
      | mark price             |
      | plan quantity          |
      | later receipt id       |
      | later receipt quantity |
      | later receipt price    |

  Scenario Outline: Schema-valid input survives durable replay unchanged
    When the native replay receives <input>
    Then the schema-valid command is durable and restores its original clocks and fields
    And its result and economics survive restart

    Examples:
      | input                 |
      | a valid fill          |
      | a future event mark   |
      | a future receipt mark |
      | a stale mark          |
      | backward clocks       |
      | an unknown order      |
      | zero cumulative       |
      | zero evidence id      |
      | an incomplete cut     |
      | extreme fill values   |

  Scenario Outline: Existing scripted replay cases remain restartable
    When the <case> script is replayed through native admission
    Then its result and economics survive restart

    Examples:
      | case        |
      | partial     |
      | cancel race |
      | unknown     |
      | halt        |

  Scenario: A fresh invalid stop count does not consume its journal path
    When a fresh CLI run requests a stop count beyond its fixture
    Then the CLI refuses without creating a journal
    And correcting the same fresh CLI command succeeds

  Scenario: A valid zero stop count still creates a resumable session
    When a fresh CLI run requests zero commands
    Then its header-only journal resumes to the valid complete result

  Scenario Outline: Invalid resume counts keep existing journal bytes
    When a CLI resume requests an invalid <bound> stop count
    Then the CLI refuses and its existing journal bytes stay unchanged
    And correcting the resume command succeeds

    Examples:
      | bound                   |
      | upper                   |
      | lower                   |
      | upper with partial tail |
