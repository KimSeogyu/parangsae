Feature: Risk-authorized offline paper execution and recovery
  The runner uses explicit session limits, costs, valuation and injected clocks.
  Paper receipts change accounting only after Risk and lifecycle checks agree.

  Scenario: An approved scripted signal reaches fixed-point accounting
    Given an explicit flat BTCUSDT paper session with 10000 USDT
    And taker fees are 5 bps and slippage is 2 bps
    When a scripted buy of 0.004 BTC is approved and paper-dispatched
    And paper fill 1 reports 0.004 BTC at reference price 30000 USDT
    Then the order is Filled with 0.004 BTC cumulative fill
    And cash is 9879.915988 USDT and fees are 0.060012 USDT

  Scenario: Partial fills settle exactly once
    Given a paper-dispatched buy of 0.004 BTC
    When paper fill 1 reports 0.001 BTC at reference price 30000 USDT
    And paper fill 2 reports 0.003 BTC at reference price 30000 USDT
    And the exact first receipt is delivered again
    Then the order is Filled with 0.004 BTC cumulative fill
    And cash is 9879.915988 USDT and fees are 0.060012 USDT
    And only two economic fills were applied

  Scenario: Changed economic evidence under the same fill identity halts
    Given a paper-dispatched buy of 0.004 BTC
    And paper fill 1 has settled 0.001 BTC at reference price 30000 USDT
    When fill 1 is delivered with a changed reference price
    Then execution is halted for conflicting evidence
    And cash, fees, position and cumulative quantity are unchanged

  Scenario: A report cannot arrive before its declared received time
    Given a paper-dispatched buy of 0.004 BTC
    When a future-received fill is presented before its delivery time
    Then no fill or fee is applied

  Scenario: Active paper valuation requires an arrived mark
    Given an accounted paper buy with a visible valuation
    When the mark reports a receipt time after the paper decision
    Then paper valuation is refused without applying session loss or changing economics
    When a delayed mark has arrived by the paper decision
    Then the arrived valuation is admitted

  Scenario: Active paper valuation refuses a future exchange event
    Given an accounted paper buy with a visible valuation
    When the mark reports an event time after the paper decision
    Then future-event valuation is refused without changing economics

  Scenario: Exact receipt replay still observes portfolio marks
    Given an accounted paper buy with a visible valuation
    When a fresh loss-breaching mark accompanies an exact retained receipt
    Then economics are unchanged and session loss is latched once
    When the next visible mark recovers
    Then session loss remains latched at the same halt epoch

  Scenario: A cancel race retains priced fills and conservative reservation
    Given a paper-dispatched buy of 0.004 BTC
    And paper fill 1 has settled 0.001 BTC at reference price 30000 USDT
    When cancellation is requested and confirmed
    And race fill 2 reports another 0.001 BTC at reference price 30000 USDT
    Then the order is Canceled with 0.002 BTC cumulative fill
    And the residual reservation is retained before the complete terminal cut
    When a complete canceled cut lists both priced fills
    Then the residual reservation is released
    And exactly 0.002 BTC is held

  Scenario: Unknown prevents further exposure and rejects quantity-only recovery
    Given a paper-dispatched buy of 0.004 BTC
    When the order times out
    Then the order is Unknown and exposure is unknown
    And a second exposure-increasing signal cannot dispatch
    When a Filled cut reports 0.004 BTC without priced receipts
    Then accounting and cumulative quantity remain unchanged
    And execution remains halted

  Scenario: Complete priced reconciliation recovers Unknown without fabricated fills
    Given a paper-dispatched buy of 0.004 BTC that timed out
    When a complete Filled cut supplies the 0.004 BTC priced receipt
    Then the order is Filled with 0.004 BTC cumulative fill
    And cash is 9879.915988 USDT and fees are 0.060012 USDT
    And execution remains halted until explicit matching reconciliation and resume

  Scenario: An approval expires before dispatch
    Given a planned buy with the explicit approval lifetime
    When dispatch occurs strictly after that lifetime
    Then the unsent order is definitively refused
    And no paper submission or fill occurred

  Scenario: Cancellation before dispatch stays local
    Given a planned buy that has not been paper-dispatched
    When cancellation is requested
    Then the order is Canceled without a paper submission
    And its real Risk reservation is released

  Scenario: Definitive paper rejection releases an unfilled reservation
    Given a paper-dispatched buy waiting for its first acknowledgment
    When the paper source definitively rejects the order
    Then the order is Rejected and its reservation is released
    And no fill or fee is applied

  Scenario: A halt invalidates a previously issued approval
    Given a planned buy at the current halt epoch
    When a halt is latched before dispatch
    Then the old approval cannot dispatch
    And no fill or fee is applied

  Scenario: Session loss cannot be cleared by resume
    Given a paper session with an explicit session loss limit
    When a priced fill breaches the frozen initial-equity loss limit
    And a matching reconciliation is followed by resume
    Then the session loss halt remains latched

  Scenario: Resume keeps the caller's observed halt epoch
    Given a paper halt with a fresh matching reconciliation
    When resume requests an older halt epoch
    Then resume is refused and the current epoch stays halted
    When resume requests the observed current halt epoch
    Then execution resumes at that epoch

  Scenario: Unsupported Expired recovery cannot be bypassed by portfolio resume
    Given an expired paper order with an unaccounted priced receipt
    When portfolio reconciliation and current-epoch resume are requested
    Then the unresolved receipt keeps unknown exposure and conservative reservation

  Scenario Outline: Restart reconstructs the uninterrupted paper result
    Given the scripted paper session has a durable ingress journal
    When the process stops at the <boundary> boundary and resumes
    Then the final cash, position, fees, fills and order states match uninterrupted execution
    And halt epoch, exposure and reservations match uninterrupted execution
    And an already settled receipt is not applied twice

    Examples:
      | boundary       |
      | partial-fill   |
      | cancel-race    |
      | unknown        |
      | halt           |

  Scenario: A damaged complete journal record prevents recovery
    Given the scripted paper session has a durable ingress journal
    When a completed input record is corrupted
    Then recovery is refused without repairing the completed records
