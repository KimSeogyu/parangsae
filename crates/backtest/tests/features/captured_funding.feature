Feature: A captured funding liability uses its economic boundary position
  These clocks, source rows, prices and rates are invented fixtures.
  Causal availability is checked at the declared application decision.
  No historical ReceivedTs, venue receipt, margin or persistence claim is made.

  Scenario Outline: A later close cannot replace the retained funding quantity
    When a captured <side> closes before a late <rate> rate arrives
    Then the original signed quantity settles once with exact fees and adverse funding rounding

    Examples:
      | side  | rate     |
      | long  | positive |
      | long  | negative |
      | short | positive |
      | short | negative |

  Scenario: Slot and use-clock admission failures preserve liability
    When wrong slot, future view and zero mark receipts try to spend captures
    Then each refusal returns its handle and cause without mutation and the corrected receipt can settle

  Scenario: A logical clone made before capture has no authority over the new reservation
    When a different account and two pre-capture forks try to spend another reservation
    Then issuer and per-capture identity refuse the foreign handles while the correct owner can settle

  Scenario: One pending slot is bounded and cannot be recaptured
    When duplicate, older and second pending slots request captures
    Then the finite pending state stays unchanged and even a known zero rate consumes only the original slot

  Scenario: Arithmetic refusal does not spend the funding handle
    When cash and cumulative funding overflow after valid capture admission
    Then both overflow paths keep every ledger field and the still-unspent funding liability

  Scenario: A post-capture staging snapshot is the same logical reservation
    When a post-capture staging clone closes and settles as its final fallible operation
    Then the published logical account has the captured payment and remains actually flat

  Scenario: The legacy current-position method retains its existing contract
    When the legacy current-position method is called only after a full close
    Then its existing current-position contract still records zero rather than captured exposure
