# C1 positive funding carry: DESIGN_CANDIDATE_NOT_PREREGISTERED, NOT_RUN

Status: design candidate, 2026-10-24. This is not a frozen preregistration.
This phase has not run C1 or computed strategy returns. It makes no edge or readiness claim.
This document grants no dataset admission or study authorization.

## Candidate mechanism

The hypothesis is positive full-pair net P&L: funding plus basis P&L minus all costs.
The proposed fully funded pair buys BTCUSDT spot and shorts USD-M BTCUSDT.
Paid cash funds the spot purchase and the separate derivative collateral.
The short requires an exact fee-net BTC match on compatible instrument grids.
Infeasible matching is refused. Fee dust or residual BTC stays recorded as `Unresolved`.
Rounding cannot erase a residual or establish an exact hedge from equal gross orders.
The hedge reduces common BTC direction exposure. Basis changes remain a risk.
Positive settled funding pays the short, but future payments can reverse sign.
Only past settled rates already admitted at the decision may inform entry.
Future final rates and settled income remain outcomes.
Model accounting must not back-credit an already-past settlement slot.
Eligibility and transaction-time deviations require an explicit contract.

A normal pair needs four fills: spot buy, perpetual sell, perpetual buy, spot sell.
Fees, slippage, basis movement, and any extra unwind fills affect net results.
Spot fees can reduce delivered BTC. Partial fee rounding can leave unresolved dust.
The two economic legs cannot be assumed to fill atomically.

## Proposed family and capital model

The proposal has two configurations, K=2, with MAXIMUM holds of 7 and 14 days.
The maximum starts at observed, admitted completion of the fee-net paired entry.
A newly admitted nonpositive settled rate would also request an early close.
The first adverse payment and all payments until actual short closure remain.
These are maximum holds. A literal fixed-14-day activity bound does not apply.

One capital pool funds paid spot, separate isolated collateral, and idle cash.
Opening the perpetual short credits no sale notional as spendable principal.
An actual sale of owned spot BTC credits its cash proceeds.
Spot gains do not automatically become margin cash. More collateral may be needed.
Both legs face basis, margin, credit, and legging risks despite the price hedge.
The proposed baseline has no borrowing or automatic transfer between wallets.
Daily and total modeled returns use full portfolio capital, including collateral
and idle cash. A funding-to-one-leg-notional ratio is a separate measure.

## Contracts still unratified

The exact entry feature, forecast-cost test, initial arming, and rearm cadence
remain proposals. A forecast over the maximum hold is not promised funding income.
Fee rates, fee assets, rounding, compatible grids, and capital allocation need review.
Wallet-local buffers and historical margin rules need their own explicit contract.
The action after a spot fill but changed hedge eligibility also needs a contract.
Source clocks, aligned inputs, missing-price behavior, and receipt assumptions
remain unresolved. Archived availability rules are not measured publication times.
Purge and CSCV exclusions must include legging and scheduled-decision delays.
Sampling floors, episode definitions, denominators, and verdict rules need freeze.
Repeated funding slots or completed pairs are not independent observations by default.

## Source admission remains blocked

The persisted funding facts lack the exchange-associated settlement `markPrice`.
The wider-history request stopped on a page of blank marks. A short successful
probe does not establish full-history coverage or account-level reproduction.
Six reviewed spot dates have source-clock quarantines:
`2020-12-21`, `2021-02-11`, `2021-04-25`, `2021-08-13`,
`2021-12-24`, and `2023-03-24`. These are not permissions to drop whole days.

Option B would choose an explicit funding-notional open-price proxy and
hypothetical isolated-margin rules. The exact dataset and field need selection.
A proxy is not an exchange-associated settlement mark or account truth.
Bounds on one fixed-quantity payment do not bound a strategy whose later actions
change with cash. Association-time and coverage assumptions need separate review.

Option C would preserve valid irregular spot timestamps and explicit quarantine
or absence events. Missing current prices would prevent new pairs and stale-price
use. Existing and one-leg exposures retain costs until an eligible actual close.
Futures risk still needs its own admitted UM mark. Unknown daily valuation cannot
become a zero return or a dropped date. Neither B nor C is an admitted data view.
Both options would support only conditional model screening after separate review.

## Claim boundary and related work

C1 is a conditional research model candidate. No performance computation is
authorized here. Unknown historical search counts cap any favorable future
development result at `INCONCLUSIVE_HISTORY`. The holdout from 2025-10-01 stays
closed. This candidate does not revise H4, rescue H1/H2, or authorize paper or live use.

The [PR 26](https://github.com/KimSeogyu/parangsae/pull/26) implementation provides the bounded synthetic accounting and lifecycle foundation. C1 remains a design candidate without a preregistration or run from this phase.

The [official funding mechanics](https://www.binance.com/en/support/faq/detail/360033525031) describe transfers between opposing holders. Current documentation does not prove historical receipt timing.

The [frozen H4 amendment](../prereg/2026-10-24-h4-causal-execution-amendment.md) governs a separate directional funding-crowding fade, not this paired carry candidate.
