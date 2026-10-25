Feature: C1 report failure preserves the real owning batch evidence
  Scenario Outline: Report acknowledgement and native result ownership remain separate
    When owning C1 finalization encounters a <fault> report sink
    Then all native unit evidence and its original ledger cause remain owned with truthful report acknowledgements

    Examples:
      | fault          |
      | healthy        |
      | open           |
      | write          |
      | file_sync      |
      | directory_sync |

  Scenario Outline: A failed outer handoff cannot replace the owned native failure
    When the actual owned C1 failure handoff encounters a <fault> sink failure
    Then the handoff is unacknowledged and both original and report errors still own every native result

    Examples:
      | fault |
      | write |
      | flush |
