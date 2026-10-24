Feature: Bounded synthetic funding carry account and lifecycle
  These scenarios use explicit synthetic prices, fees, wallets and margin assumptions.
  They make no historical liquidity, solvency, strategy profit or production journal claim.

  Scenario: Four execution sides conserve one capital allocation
    Given one explicit capital allocation to spot, isolated futures and idle cash
    When all four entry and exit executions complete at 1000 USDT
    Then the closed wallets total 9997.002 USDT after every execution fee

  Scenario: A purchased spot leg cannot create futures cash
    Given a visible completed spot purchase with 0.999 BTC after fees
    Then the unhedged spot leg has spent 1000 USDT and has created no futures cash

  Scenario: A late funding receipt uses the quantity captured at settlement
    Given a separately completed short hedge of the owned 0.999 BTC
    When a one-basis-point funding receipt arrives only after both legs have closed
    Then funding is 0.0999 USDT from the retained quantity rather than the zero current short

  Scenario: An unknown short close cannot authorize spot liquidation
    Given a separately completed short hedge of the owned 0.999 BTC
    When the short close times out before any priced close receipt
    Then the spot exit is refused and both leg liabilities remain visible

  Scenario: Pair equity does not replenish an isolated wallet
    Given a separately completed short hedge of the owned 0.999 BTC
    When both visible marks rally enough to breach the isolated margin scenario
    Then positive pair equity does not move spot gains or idle cash into isolated collateral

  Scenario: A stranded leg keeps its inventory and costs
    Given a visible completed spot purchase with 0.999 BTC after fees
    When the one-leg deadline expires while no hedge can be evidenced
    Then the trial is unresolved with the paid spot inventory and reserved hedge capital

  Scenario: An exact receipt remains idempotent at capacity
    Given a hedged session with capacity for one funding identity
    When the same accounted funding receipt is replayed at full retention capacity
    Then no second funding credit or identity is created

  Scenario: Fresh reconstruction uses data rather than serialized authority
    Given a separately completed short hedge of the owned 0.999 BTC
    When only the original session and data commands reconstruct a fresh engine
    Then wallets, order progress, retained liabilities and halt epoch match the uninterrupted engine
