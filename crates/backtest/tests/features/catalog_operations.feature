Feature: Typed offline catalogue operations are proposals, not orders
  Scenario: Source catalogue preserves every distinct family and role
    When I inspect the eighteen source families
    Then each source number is unique and complete

  Scenario: The current BTC USD-M domain refuses unsupported markets
    When I request option assessment in the current BTC USD-M domain
    Then the catalogue refuses the unsupported option domain

  Scenario: Inventory-capped market making is a noncrossing proposal
    When the synthetic quote inventory is at its long limit
    Then only the inventory-reducing ask is proposed

  Scenario: TWAP conserves integer venue lots
    When I split ten lots into three equal-time slices
    Then the schedule has quantities four three three and exact times

  Scenario: VWAP uses a previously available profile
    When I split ten lots using a known one two one volume profile
    Then the schedule has quantities three five two

  Scenario: Risk parity states its diagonal risk model
    When two synthetic assets have volatility one hundred and two hundred
    Then their quantized allocation is two thirds and one third

  Scenario: A volatility regime uses hysteresis
    When causal volatility changes from high through middle to low
    Then the defensive regime persists until the low threshold

  Scenario: Refused original strategy input remains correctable
    When an existing fade strategy receives a zero-price bar then its correction
    Then its corrected output matches a fresh strategy

  Scenario: Insufficient weight precision refuses instead of dropping an asset
    When extreme inverse-volatility weights cannot remain positive
    Then the allocation reports a typed resolution refusal
