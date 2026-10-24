# C1 positive-funding carry protocol

Status: FROZEN_SEMANTIC_NO_EXECUTION_GRANT.
Semantic confirmation: Engineering Lead, 2026-10-24T16:09:46.737580+00:00.
No C1 strategy outcome has been read or produced.
Protocol version: `C1-2026-10-24-v1`.
Intended repository path: `research/prereg/2026-10-24-c1-positive-funding-carry.md`.

This reference records the closed pre-outcome semantic choices. It does not
bind an actual source revision, build, complete input pin, native control, or
launch grant. Those execution bindings belong to a separate EL envelope.
The machine-readable companion has logical path
`research/prereg/2026-10-24-c1-canonical-settings.json`.
Its complete settings and role preimages define the frozen semantic identities.
Any substantive change requires new identities and review before outcomes.

## Claim and candidate settings

C1 tests conditional full-capital net return from paid BTC spot and an equal
fee-net BTC short in a linear USDT perpetual. Positive past funding supplies an
entry premise. Future funding, basis changes, and execution costs are outcomes.
No future rate is locked. Equal BTC does not establish either wallet's solvency.

The only candidate contrast is maximum hold: grid 0 uses 168 hours, and grid 1
uses 336 hours. The maximum starts at observed complete matched entry
`hedged_at`. Physical last-entry-fill time and knowledge lag remain separate
report fields. A nonpositive newly known settled rate can end the hold earlier.

The estimand is the selected policy on the declared conditional model snapshot.
It is not historical exchange-account reconstruction, actual execution capacity,
intraminute liquidation, risk-free yield, or statistical promotion.
The sealed H1, H2, and H4 studies remain unchanged.

## Source and clock contract

The allowed interval is `[2020-12-18T00:00:00Z, 2025-10-01T00:00:00Z)`.
The C1 source contract uses the existing 1,742 normal spot partitions unchanged
after separate source admission.
Six exceptional dates use valid daily rows only. Monthly rows are audit-only.
Price-free Gap and Quarantined records retain unavailable source periods.
The fixed policy is `C1_ARCHIVED_SPARSE_KEY_CERTIFIED_V2`. Its six daily-only
rebuilt dates are 2020-12-21, 2021-02-11, 2021-04-25, 2021-08-13,
2021-12-24, and 2023-03-24. Ten spot status dates comprise those six plus
2020-12-25, 2021-03-06, 2021-04-20, and 2021-09-29. Their exact machine gap
keys come from the Data-owned certified profile, not transcribed clock labels.
UM trade and funding allow no gaps or quarantined keys. Perpetual mark allows
only open keys 1723456920000000 and 1723456980000000 as Gap, noticed at
1723456980000000 and 1723457040000000. These are not carried prices.
No timestamp clamp, fabricated bar, full-day omission, or shorter window is used.

The planned DATA set has 7,003 objects: 5,245 common core, 1,742 normal spot,
six rebuilt spot, and ten spot status objects. `c1-source-manifest.json` is an
additional pinned source-manifest object. Existing `source-catalog.json` is a
DISTINCT opaque Contract that preserves source lineage and the 30 shared-key
payload conflicts. Retain its exact bytes. Never inline it into CanonJson,
replace it with the DATA-only manifest, or delete the existing view member.
DATA excludes both metadata objects and the protocol or control. With this
prereg and its canonical settings Contract inserted, the closed pin has 7,007
objects: 7,003 DATA plus four nonDATA objects. Planned shape does not establish
the actual copied pin; that execution binding remains separate.

Funding uses the admitted consecutive eight-hour UTC grid. Its modeled
availability is at least settlement plus 60 seconds. That floor is not measured
historical publication latency. The funding notional uses `mark1m.open` at the
same settlement key as an explicit proxy. It is not the missing associated
exchange charge mark. The risk rail uses `mark1m.close` with original availability. The first financial
profile is `C1_FUNDING_FLOOR60_SUPPORTED_V1`. Every declared settlement must
support the rate and same-key positive mark-open proxy by S plus 60 seconds,
regardless of exposure. Each captured receipt is attempted exactly at S plus
60 seconds with then-causal facts. Later source availability is unsupported,
not clamped or rescued by a future quote. This global admission and runtime
rule does not assert measured historical publication latency.
Preserve original stored event, raw_calc_time, and availability. Semantic raw
settlements snap to the nearest eight-hour key within one second. Effective
availability is no earlier than the original event. The current writer's event
and raw_calc equality is not a certificate for every historical writer. No event
is rewritten to manufacture equality.

The audit-only MONTHLY payload differs from DAILY on 28 shared valid keys on
2020-12-21 and two on 2021-12-24. The other four exceptional dates have zero
observed shared-valid differences. DAILY-only precedence was selected before
outcomes. This is not source-payload parity or a winning-true-price claim.
MONTHLY never heals DAILY and no post-result source switch is permitted.

Actor delivery is `ceil_to_1ms(effective AvailableTs)`, with no extra microsecond.
Original EventTs, AvailableTs, and collector ReceivedTs remain provenance.
Synthetic actor ReceivedTs has a separate tag. A dependent hedge or spot-exit
decision uses the next millisecond strictly after the complete cut is admitted.
At settlement S, admitted UM receipts and cuts drain before the rate-free
funding boundary captures the actual pre-S short. Ambiguity is technical failure.

Financial commands require the latest expected visible Data context. Source-open
age is at most 120 seconds. C1 inherits no H4 mark-gap carry exception.
Mandatory risk Observe occurs at each UTC minute plus 1,000 microseconds while
actual inventory, nonterminal orders, unsettled liability, or a nonidle native
phase exists. A retained pair pointer alone does not make a closed account active. Required missing
input stops the whole trial. Idle reconciled cash can skip an unused quote.
This is sampled modeled risk, not intraminute liquidation evidence.

Entry decisions occur at each UTC hour plus 1,000 microseconds. Both trade
references use that hour's preceding final one-minute key and positive count.
Each original availability must be no later than the decision.
The ordinary as-of risk context is not an aligned entry basis.

## Entry and rearm

One generated SGDv2 opportunity control, held by the private backtest actor,
owns AwaitFalse, armed, reserved, busy, disabled, and stopped mode.
Signal owns only the causal rate window and key-classification payload.
Native key, clock, quote, and economic checks precede generated transitions.
CarryEngine and CarryLedger remain the sole lifecycle and money owners.

Independent train initialization starts AwaitFalse. A new OOS fold activates an
AwaitFalse epoch only from healthy fully settled idle state. It preserves cash,
the original loss anchor, all history, source watermarks, and consumed keys.
An active or stopped account cannot reset or replay a consumed key.
For new entry and rearm, effective funding S must satisfy
`S >= native_entry_epoch_start` and `S > lastConsumed`. The epoch start is the
actual healthy settled policy or source NewEpoch activation clock. A pre-epoch
unconsumed Seen key is suppressed as Keep with no generated event. It is not
Replay, NoSize, or a fabricated consumed key. Older causal rates remain R9
history. NewEpoch never advances consumed keys. A midnight S equal to activation
can qualify only after its actual availability. Owed funding and required exits
are not suppressed by this entry gate.

The actor admits the complete causal batch before selecting its newest funding
key. Earlier newly arrived keys become SupersededForEntry. Temporary missing
terms remain Pending until the first completed evaluation or a newer key.
A definitive capital or grid refusal consumes the key without false evidence.
Occupied keys cannot rearm or become fresh entries after closure.
A source discontinuity resets AwaitFalse or fails the source contract.

A complete known-false predicate while healthy idle arms a later key.
A pass while armed emits one proposal, consumes that key, and disarms before fill.
No same-key retry, pyramiding, or continuous-positive-regime churn is allowed.
The predicate requires all of the following:

- The latest admitted settled rate is positive.
- The nine latest semantic settlements are consecutive eight-hour rows.
- The aligned perpetual-minus-spot reference is nonnegative.
- `q * P_ref * ((H / 72) * R9)` strictly exceeds exact headline `C4`.

`R9` is the sum of those nine settled rates. Their first and last keys are 64
hours apart. Nine eight-hour payments supply the 72-hour normalization.
The projection is a full-maximum-horizon persistence scenario, not expected
funding under the earlier exit. The projection never becomes spendable cash.
`C4` estimates four fills using current causal references, fee assets, grids,
and headline costs. It does not read future prices or realized exit costs.
The estimate reads the current scenario account without mutation. The
hypothetical perpetual cash change is
`floor((R_scaled + q * (headSell - headBuy)) / SCALE) - realized_perp`.
`R_scaled` and `realized_perp` are the current known cumulative ledger fields.
This preserves their fractional residue across pairs and OOS folds. The spot
gross-to-fee-net round trip and both perpetual fees use exact headline inputs.
A standalone rounded loss, cleared residue, or another scenario's residue is
not this estimator.

## Capital and assumed trading terms

All money and quantity use checked fixed point at scale 100,000,000.
The following numbers are research assumptions. They are not user preferences,
verified historical filters, historical margin tiers, or actual account fees.

| Item | Value |
| --- | ---: |
| One initial total capital | 1,000,000 USDT |
| Spot wallet | 475,000 USDT |
| Isolated perpetual wallet | 475,000 USDT |
| Unavailable idle wallet | 50,000 USDT |
| Spot close reserve inside spot wallet | 25,000 USDT |
| Perpetual close reserve inside its wallet | 5,000 USDT |
| Funding reserve inside perpetual wallet | 20,000 USDT |
| Initial entry envelope per leg | 450,000 USDT |
| Unhedged notional ceiling | 500,000 USDT |
| Assumed initial margin | 100% of entry notional |
| Assumed maintenance | 1% of current notional plus provisions |
| Sticky loss control | 5% of original account capital |
| Spot quantity step | 0.00001 BTC |
| Perpetual quantity step | 0.001 BTC |
| Gross spot quantity ceiling | 100 BTC |
| Net perpetual quantity ceiling | 99.9 BTC |
| Minimum notional per order | 100 USDT |

Local reserves are designations inside wallets, not additional equity.
The idle wallet cannot rescue either trading wallet. Opening the perpetual short
credits no spendable sale principal. There is no borrow, transfer, interest,
automatic reinvestment, or matched-size rebalance.

The private sizer selects the largest exact compatible quantity within current
admitted wallet cash and the original entry envelopes. The 450,000 USDT
perpetual entry cap includes its fee even when a later wallet is richer. Profits do not raise the
envelopes. Losses can reduce quantity. Gross spot, net spot exit, and net
perpetual quantities must all fit their grids. The actual net amount is gross
spot less the checked upward-rounded BTC fee, not a rounded 0.999 multiplier.
The 100 USDT minimum is the existing Plan and Dispatch admitted-context proxy.
At sizing, both the fee-net spot-exit quantity and net perpetual quantity must
meet the known 100 USDT minimum at the current trade and risk references.
That checks known terms. It does not guarantee a future fill minimum or add an
actual-fill filter. Only the selected price band is added at the driver boundary.
A partial receipt cannot erase dust or assert a complete hedge.

Spot buy fee is 10 bps in BTC. Spot sale fee is 10 bps in USDT. Perpetual fees
are in USDT. Each fee rounds against the trader to the relevant asset tick.
No discount is assumed. A cap or arithmetic failure retains state and stops the
whole trial technically. The margin and loss controls are sticky, sampled study
controls. They do not guarantee a maximum realized loss.

## Execution and price band

The historical venue proxy is full-fill-or-pending. It does not estimate capacity
or partial-fill probabilities from minute volume. Each leg independently uses
the first forward eligible positive-count minute open strictly after dispatch.
No common future liquidity choice supplies both fills.

Native prices retain full 1e-8 precision. No 0.01 USDT final-fill grid is selected.
The driver freezes a two-sided eligibility band around its causal reference R:
`lower = ceil(R * 9900 / 10000)` and `upper = floor(R * 10100 / 10000)`.
The prospective modeled price uses the existing `fill_price` from the raw open.
Only a price inside the band is eligible. The emitted reference remains the raw
open, so slippage is not applied to an already adjusted price.
Session constants are approval age 120,000,000 microseconds, static dispatch
budget 1,000 microseconds, and native MaxAttempts(1). This setting is not the
global trial or invocation count. ExecutionConfig uses the same
approval age. Plan and Dispatch share their decision clock. Dormant NotFound
uses 5,000 ms receive window plus 1,000 ms margin, totaling 6,000,000 microseconds.
The dispatch budget is not measured RTT or a deadline for bar receipts.
NotFound never proves order absence or supplies a zero or cancel cut.
Native `max_price` is the upper bound for every side. The driver does not pass
the lower sell band as that native bound. This band is a conditional model rule,
not an asserted Binance order type.

Entry is spot first. A complete fee-net spot cut permits a new later hedge
decision. Worsened economics alone does not erase the paid spot or prevent a
supported risk-reducing hedge. Fresh safety, inventory, source, and capital
checks still apply. Exit is perpetual first, then spot after complete short-flat
evidence. Only supported reduction before the original deadline is permitted.

Opening and closing each have one original 600-second deadline. Normal complete
evidence at that deadline is allowed. Unsupported state or unfinished deadline
stops the whole trial. No new cleanup budget or post-deadline fill is invented.
Modeled perpetual fills avoid the 15 seconds on either side of a nominal funding
settlement. This is a declared proxy guard, not historical eligibility proof.
Every funding settlement captured before physical short closure remains owed
until its receipt, including a receipt delivered after physical flatness.
Drain already admitted or due cash facts before classifying a new funding key.
Safe BeginExit never waits for an unpublished or not-yet-due S+60 payment.
Any claimed Closed or settled-cash state requires all inventory, orders,
complete cuts, and captured receipts to be resolved. Healthy idle additionally
permits rearm. Genuinely settled Stopped cash is valid for valuation and
boundary completion, and it remains Stopped. Cash valuation or a payment never
clears the stop latch.

## Continuous OOS and fixed terminal actions

Each cost scenario has one continuous OOS account. Each independent train window
can initialize its own declared capital. OOS folds do not replenish wallets,
reset the original loss anchor, clear latches, or remove outstanding history.
A stopped account stays stopped after a supported close.

At every train or test end E, a predeclared terminal intent begins at E minus
600 seconds. New entry then stops. Actual orders and held legs close through
the sequential actor. Closed or settled cash, with all liabilities resolved,
must be known by E. Genuinely settled Stopped cash also qualifies and stays
Stopped. Health is a rearm condition, not a waiver of settlement requirements.
Drain already admitted receipts and cuts with R <= E before that inclusive
knowledge check. Economic, source, and physical fill keys remain strictly < E.
No new order is submitted at E. Unpublished evidence with R > E cannot rescue
completion. E plus one millisecond is only the final valuation and evidence
flush, never an extension for an unfinished close. Missing liquidity cannot
select an earlier terminal time or create a backdated close.
No per-fold H-hour entry ban is added. Terminal exits retain every cost and do
not count as normal pairs. A global development-end entry guard requires enough
time for opening, observed MAX, next-hour rounding, and complete closing before
the final terminal cut. It cannot assume a future favorable early exit.

## Exact calendar and train-only selection

All ranges below are UTC half-open. The feature warmup starts on 2020-12-18.
Train starts on 2020-12-21. Each train has 365 days. A fixed one-day calendar
separation precedes its test. Test and step are 90 days. The native remainder merge threshold is 45 days.
The fixed 29-day remainder joins the last full test. OOS has 1,379 calendar days and 15 folds. The last test has 119 days.

| Fold | Train start .. exclusive end | Test start .. exclusive end | Test days |
| --- | --- | --- | ---: |
| 1 | 2020-12-21 .. 2021-12-21 | 2021-12-22 .. 2022-03-22 | 90 |
| 2 | 2021-03-21 .. 2022-03-21 | 2022-03-22 .. 2022-06-20 | 90 |
| 3 | 2021-06-19 .. 2022-06-19 | 2022-06-20 .. 2022-09-18 | 90 |
| 4 | 2021-09-17 .. 2022-09-17 | 2022-09-18 .. 2022-12-17 | 90 |
| 5 | 2021-12-16 .. 2022-12-16 | 2022-12-17 .. 2023-03-17 | 90 |
| 6 | 2022-03-16 .. 2023-03-16 | 2023-03-17 .. 2023-06-15 | 90 |
| 7 | 2022-06-14 .. 2023-06-14 | 2023-06-15 .. 2023-09-13 | 90 |
| 8 | 2022-09-12 .. 2023-09-12 | 2023-09-13 .. 2023-12-12 | 90 |
| 9 | 2022-12-11 .. 2023-12-11 | 2023-12-12 .. 2024-03-11 | 90 |
| 10 | 2023-03-11 .. 2024-03-10 | 2024-03-11 .. 2024-06-09 | 90 |
| 11 | 2023-06-09 .. 2024-06-08 | 2024-06-09 .. 2024-09-07 | 90 |
| 12 | 2023-09-07 .. 2024-09-06 | 2024-09-07 .. 2024-12-06 | 90 |
| 13 | 2023-12-06 .. 2024-12-05 | 2024-12-06 .. 2025-03-06 | 90 |
| 14 | 2024-03-05 .. 2025-03-05 | 2025-03-06 .. 2025-06-04 | 90 |
| 15 | 2024-06-03 .. 2025-06-03 | 2025-06-04 .. 2025-10-01 | 119 |

Only headline train daily net Sharpe selects the next fold horizon.
Defined positive Sharpe is eligible. Ties select the shorter horizon.
If neither is eligible, the next fold is FLAT. Later fold choices remain
unavailable to earlier decisions. FLAT keeps actual cash and calendar days.
It cannot turn liabilities into flat cash.

The one-day separation follows resolved train-end outcomes. It is a fixed extra
calendar separation, not a universal independence guarantee. Shared past causal
features do not automatically require purging their full lookback.

## Three cost-policy executions and identities

The only cost paths are the following predeclared accounts.

| Role | Perpetual fee per fill | Slippage per fill | Use |
| --- | ---: | ---: | --- |
| OOS_HEADLINE | 5 bps | 2 bps | Primary |
| OOS_HIGHER_PERP_FEE | 6 bps | 2 bps | Necessary local screen |
| OOS_SLIPPAGE_STRESS | 5 bps | 5 bps | Report only |

All retain spot fee 10 bps, the headline 5-fee/2-slippage opportunity estimator,
and the headline train-selected horizon schedule. Actual scenario fees and
capital states can change feasible quantities, event timing, and subsequent
choices. This is not a fixed-trade accounting subtraction. Realized-price PnL
already contains adverse execution. Attribution does not subtract it twice.
Report-only stress means a valid numeric stress loss is not a hard gate. Any
technical unit, including stress, stops the whole invocation. Keep completed
unit metrics and actual ledger starts. Overall verdict is
INCONCLUSIVE_TECHNICAL with completion INCOMPLETE and the failed role. No
strategy pass, automatic retry, zero padding, or partial-window rescue follows.

Two training-config identities and three continuous policy identities are charged
conservatively, at most five initial identities. Fifteen train windows per
candidate plus three continuous accounts produce 33 segment units and 66
start/terminal records only if complete. This is one bounded invocation, not five
searched horizons. No automatic retry or per-config OOS curve grid is added.
Original H4 accounting is unchanged.

Config identity binds this version, canonical study settings, and semantic role.
Policy identity also binds ordered candidate identities, the selection contract,
and actual cost scenario. Native trial preimage remains exactly
`{attempt, config_id, pinned_root}`. Declared local attempt label 4 follows H4 attempt 3;
it is not proof of the global attempt maximum. Source, binary, model, protocol,
input, output claim, and fresh grant remain mandatory separate run bindings.
The semantically frozen packet contains reproducible study and role identities.
They are not trial IDs or launch authority. Trial IDs remain unbound until the
complete input pin is independently bound in the later execution envelope.
RunUnit identity hashes exactly `{schema, study_id, attempt, config_id, unit_kind, fold}`.
The schema is `parangsae-c1-run-unit-v1`. Here `study_id` means the final settings
identity. Train uses folds 1 through 15. A continuous OOS unit uses null fold.
Input pin, source, binary, and output bind in a separate accepted envelope.
Pinned settings bytes do not acquire a circular self-referencing input pin.

## Returns, dependence, and verdict

Daily equity includes spot free cash, net spot value, isolated cash, perpetual
unrealized PnL, and disjoint idle cash. Reserves are not added again.
Each session uses exact final spot and mark keys at the next-midnight-plus-1ms
report cut. Event labels do not make those prices knowable earlier.

`r_d = E_d / E_(d-1) - 1` uses the same continuous account. Opening prior equity
is the original C0. Total return is `E_final / C0 - 1`, reconciled to the daily
chain with declared integer rounding. A sum of reset-train cash is not OOS wealth.
Every idle, legging, closing, and forced-cost day remains. A missing indispensable
owned value is technical failure, not a dropped or zero-return date.
The sole comparator is the same capital held in zero-interest USDT cash.
No BTC, S01, or additional traded control is included in this bounded invocation.

Necessary local project screens are positive headline net return and Sharpe,
positive higher-fee Sharpe, DSR at least 0.95, PBO at most 0.20, and 50 normal
completed matched pairs across at least 10 test folds. Daily Sharpe uses calendar
T=1,379, ddof 1, and sqrt(365). Normal activity requires both legs closed by a
strategy MAX or nonpositive-rate exit. Terminal, risk, partial, and failed pairs
retain money but cannot inflate activity. The activity floor does not prove
independence or statistical power.

The two-column IS matrix uses the earliest-covering declared train window.
It retains the first 365 days, then only each later chronological new suffix
based on the previous train end, for 1,625 rows. Preserve source fold, date, and
column provenance. Reset train accounts can disagree on overlapping-date
returns. The convention does not compare or prove those returns equal.
CSCV uses 16 contiguous blocks, all 12,870 half/complement splits, and the
explicit native `pbo_ticks(columns, 16)` gap. The legacy C1 family default of
one day is not used. Native `is_days` trims the first 16 IS rows next to a left
OOS block and the last 16 next to a right OOS block. The half-open kept range
saturates to empty. OOS rows and boundaries between two IS blocks are untrimmed.
The local exposure bound is opening 600 seconds plus MAX 14 days plus less than
one hour of scheduling plus closing 600 seconds. Round that 14-day-80-minute
bound to 15 days and add one declared guard day. The causal three-day rate
feature is not automatically added. This guard does not remove persistent
wallet state or prove regime mixing. PBO describes the fixed candidate matrix.

DSR reuses the native integer normal approximation and measured population
central moments with non-excess kurtosis. Cross-config annual Sharpe variance
uses ddof 1, treats an undefined column Sharpe as zero, and has floor 0.25.
The native probability gate is inclusive at 95,000,000 ticks on the 1e8 scale.
An independent true-CDF diagnostic cannot replace that gate after results. Before C1, history contains 85 observed identities, 13 documentary
charges, and irrecoverable unknown U. Keep the 202 floor and report exactly how
it is implemented. The floor is not an exact global or independent trial census.
Every actual failed, aborted, or retried identity remains visible.

Report signed funding, basis, fees, asynchronous exposure, and forced contributions
on consistent denominators. Zero funding without exposure is no observation of
the income mechanism. The earlier proposed 28-day and 56-day block bootstrap is
explicitly DEFERRED_NOT_COMPUTED before outcomes. This first study promises no
block interval, PRNG, or postprocessor. The deferred diagnostic was not a hard
gate. Its absence does not resolve unknown prior search or change the positive
INCONCLUSIVE_HISTORY ceiling. Calendar, pair, funding, and fold counts remain
separate from independent observations.

Numeric gate results are Pass, Fail, or Undefined. An undefined statistic is
not a valid numerical failure. Idle zero return without strategy exposure is
Undefined, not an observed failed trade. For actual completed observed pairs,
use the exact net cash sign for the return comparison even if displayed ratio
rounding reaches zero.

Keep the raw mechanical PBO and existing RANK_LAST and MIDRANKS rules. If both
complete IS candidate columns have undefined native full-column Sharpe, there
is no finite global rank evidence. Report the raw number but mark its PBO gate
Undefined. If any full-column Sharpe is defined, retain existing PBO validity,
including finite-tie and other numerical failures. No helper is weakened.

Input, source, clock, model, arithmetic, or unresolved deadline failure takes
precedence as TECHNICAL or INCOMPLETE. A valid decisive local numeric failure
can REJECT_ON_DECLARED_MODEL_SNAPSHOT, with sparse support disclosed.
Without a valid numeric failure, undefined statistics or coverage-only
shortfall are INCONCLUSIVE_SPARSE. Favorable numbers remain
INCONCLUSIVE_HISTORY. No result authorizes paper, holdout, keys, or live orders.
No post-result horizon, source, fee, threshold, or PBO optimization rescues C1.

## Finite bounds and preflight

The eight-hour calendar bounds each continuous account by N=4,137 settlement
keys and each 365-day train by N=1,095. There are at most N candidate attempts,
4N single-attempt leg orders, 4N full-fill receipts, and N funding identities.
These are calendar bounds, not observed profitable opportunities. Partial
synthetic fixtures use a separate bound of four receipts per order, so their
receipt capacity is 16N rather than the historical full-fill model's 4N.
A bound failure retains state and is technical. It never prunes history or
starts a fresh account.

Before RunStarted, the separate accepted envelope must bind the reviewed source,
binary, generated control, protocol artifact, complete admitted input pin,
interpreter schema, claimed fresh output, and launch grant. Missing or unbound
execution values refuse execution. Native behavioral evidence and independent review remain
required. Semantic freeze creates no dataset, study, or execution grant.

## Semantic and execution freeze boundary

The eventual complete input pin includes this prereg and the semantic settings
file. Neither document contains that final pin or a claimed actual build.
The DATA-only `c1-source-manifest.json` lists neither itself nor control bytes.
The distinct `source-catalog.json` remains an opaque pinned Contract with its
existing bytes and lineage. No metadata is dropped to obtain a smaller pin.
Root separately binds actual source, binary, generated control, complete inputs,
prereg byte digest, output claim, and activation before RunStarted.
Semantic confirmation alone never authorizes a run.
