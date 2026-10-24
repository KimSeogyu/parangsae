Feature: Repeated synthetic pairs retain one session and its settlement history
  This observes the existing CarryEngine public API only.
  It does not implement a C1 strategy, historical runner or production journal.

  Scenario: A second pair cannot reset capital or erase a late settlement
    When one engine completes two pairs and receives the second funding payment after physical flat
    Then the second pair retains the first cash, fees and original loss anchor
    And zero positions cannot rearm while the captured settlement is undrained
    And the late negative payment retains its original settlement and captured quantity
    And duplicate old identities preserve both pairs at full retention capacity
