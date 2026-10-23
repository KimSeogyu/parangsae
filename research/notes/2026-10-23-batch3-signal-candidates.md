# Batch-3 signal candidates — 2026-10-23

Design proposals only: five BTCUSDT USD-M families, four configurations each.
No data analysis, event count, backtest, sweep, holdout read, or strategy
implementation was performed. Thresholds below are proposed constants, not
estimates from returns. None has evidence of edge.

## Evidence and scope

Repository baseline: `973cff653cde011b6fd7a5a58496a8802e2918f3`.
`research/knowledge/` is absent on that baseline. I read the locally available
research branch at `a6c6ced48cbb1f8d42108bcb2183a566c2846df8`, without merging it:

- [Hub and handoff](https://github.com/KimSeogyu/parangsae/blob/a6c6ced48cbb1f8d42108bcb2183a566c2846df8/research/knowledge/developer-handoff.md): distinguish mechanisms, input availability, and demonstrated results.
- [DNP synthesis, R02–R05](https://github.com/KimSeogyu/parangsae/blob/a6c6ced48cbb1f8d42108bcb2183a566c2846df8/research/knowledge/investor-dnpquant/research-synthesis.md): exhaustion need not cause a rebound; funding carry and delayed news reaction need separate input and cost evidence.
- [Alternative signals](https://github.com/KimSeogyu/parangsae/blob/a6c6ced48cbb1f8d42108bcb2183a566c2846df8/research/knowledge/alternative-signals.md), [community map](https://github.com/KimSeogyu/parangsae/blob/a6c6ced48cbb1f8d42108bcb2183a566c2846df8/research/knowledge/community-map.md), and [PIT validation](https://github.com/KimSeogyu/parangsae/blob/a6c6ced48cbb1f8d42108bcb2183a566c2846df8/research/knowledge/public-data-pit-validation.md): discovery, publication, reception, and later revisions are different events.

These are idea and data-contract sources, not independently reproduced trading
results. [Queue v2](../strategy-queue-v2.md), the
[H4 preregistration](../prereg/2026-10-23-h4-funding-crowding-fade.md),
[H5 preregistration](../prereg/2026-10-23-h5-premium-reversal.md), and
[premium dataset sign-off](2026-10-23-h5-premium-dataset-signoff.md) supply the
local input contracts and overlap checks. Issue
[#24](https://github.com/KimSeogyu/parangsae/issues/24) is addressed separately
below; its full issue body was unavailable offline. The task's description and
the hub's reference establish the limited premise used here.

## Shared point-in-time and trading contract

- All times are UTC. Proposed research bounds remain
  `[2020-12-18 00:00, 2025-10-01 00:00)`; no values were read to write this note.
  Warmup must fit inside those bounds. No entry whose planned exit crosses the
  development boundary or a fold's permitted entry boundary is admitted.
- A UM minute with open `u` has event close `u + 60 s - 1 ms` and availability
  `event close + 1 ms`. A premium minute is available at `u + 60 s`.
  Settled funding at event time `s` is admitted at
  `max(stored AvailableTs, s + 60 s)`. These are the existing historical
  read-path assumptions, not measured live transport latencies. Actual
  `ReceivedTs`, when available, also gates replay; never invent an old reception
  time from an archive download today. Live or prospective replay admits only
  after both availability and reception.
- `C(t)` is the close of the valid UM minute ending at boundary `t`;
  `R(t,L) = C(t)/C(t-L) - 1`. Hour bars cover `[t-1 h,t)` and carry the maximum
  constituent availability. An hourly decision is `t + 1 ms`; settlement-based
  decisions use their specified boundary plus 1 ms. Every dependency must pass
  `Observation::causal_at(decision)` and be consumed through `Causal<T>`.
- `M(t,L)` is the arithmetic mean of valid premium minute closes in `[t-L,t)`,
  in ratio units (`ticks / 1e8`), never price units. All required windows are
  complete consecutive grids with `count > 0`; missing, excluded, all-zero-OHLC
  premium, or filler minutes make the feature unavailable. No interpolation or
  carry across a gap. This deliberately requires more complete windows than
  H5's daily minimum. Coverage loss must be reported, not silently relaxed.
- `sd` means sample standard deviation (`ddof = 1`); a zero denominator makes
  the feature unavailable. `Q_q` is sorted observation number `ceil(q*n)` using
  one-based indexing. All baselines exclude the candidate interval explicitly
  identified below. Equality at a strict threshold does not qualify.
- A family has one position, no pyramiding, no extension, and ignores signals
  while occupied. The only exit is elapsed hold time `H` from actual entry,
  requested at the next hourly decision; fold/data-end flattening remains a
  separately counted forced exit. Entry/exit fills use the first tradable UM
  minute open at or after the decision, with existing refusal handling. An open
  at `t` cannot fill a decision at `t + 1 ms`. No high/low fills or assumed stops.
- Proposed paper sizing is a fixed quantity from a signed 1x equity-notional
  target at entry, floored to 0.001 BTC, held until exit. Price/Qty and accounting
  remain checked fixed-point types; ratios/statistics do not change money types.
  Headline assumptions are 5 bps fee plus 2 bps slippage per side, all actual
  funding cashflows included; report 6+2 and 5+5 sensitivities as well. Minute
  bars cannot establish actual spreads, depth, or market impact.
- For hourly conditions, enter only on a false-to-true transition. Following
  an exit, that condition must be observed false while flat before rearming.
  Unavailable inputs neither trigger nor rearm. Funding-event families consider
  each settlement ID at most once, even after a restart. All feature buffers have
  finite caps derived from the stated windows; no expanding history.

## B3-01: funding-tail premium relief

**Hypothesis.** After an unusually one-sided settled funding rate, a confirmed
premium contraction and price turn identify residual crowding unwind. This
tests short-lived relief after settlement, rather than H4's daily funding-level
fade or H5's daily premium-level fade.

**Features and rule.** At each observed settlement `s`, use its final rate `f_s`
and the preceding 270 consecutive 8 h settlement rates, excluding `s`.
Require the archived 00/08/16 grid; an unexpected cadence or missing slot makes
the event unavailable. Let `a = sign(f_s)`. A positive rate must be strictly above
`Q_0.90`; a negative rate strictly below `Q_0.10`; zero never qualifies.
Freeze that tail test using inputs available at `s + 60 s`.
At decision `s + 30 min + 1 ms`, compute
`D = M(s+30 min,30 min) - M(s,30 min)` and `r = R(s+30 min,30 min)`.
For scale `v`, use `sd` of 672 prior, consecutive hourly differences
`M(j,30 min) - M(j-30 min,30 min)`, at hourly boundaries
`j = s-671 h, ..., s`. Require `a*D < -k*v` and `a*r < 0`.
Enter target sign `-a`. All post-settlement minutes and the delayed funding row
must have been admitted by that decision; otherwise discard this settlement.

**Grid.** `k ∈ {1,2}` × `H ∈ {4 h,12 h}` = **4 configurations**. Funding quantiles,
270 observations, 30 min confirmation, and 672 scale observations are fixed.

**Expected failure.** Funding tails can be tied near a baseline or extremely
sparse; a brief relief can precede a stronger trend. Confirmation can consume
the whole move, while paid costs exceed the remaining premium-sized effect.

**Data needed.** UM OHLC/count, settled funding with effective availability,
and premium minute closes/count. Premium requires a causal engine stream even
though a dataset sign-off exists. Warmup: at least 90 d for funding and 28 d
plus 30 min for the scale, all complete.

## B3-02: premium impulse before price expansion

**Hypothesis.** A sustained change in leveraged demand can first appear in the
premium while the outright price remains quiet, then precede directional BTC
movement. This deliberately predicts continuation, unlike H5's premium fade.

**Features and rule.** Decide hourly at `t + 1 ms`. Define
`D(t) = M(t,4 h) - M(t-4 h,4 h)` and `r4 = R(t,4 h)`.
For `j = t-679 h, ..., t-8 h` (672 hourly boundaries), compute historical `D(j)`;
let `mu_D = mean(D(j))` and `v_D = sd(D(j))`.
Let `v_r = sd(R(j,1 h))` on those same boundaries.
Define `z = (D(t)-mu_D)/v_D`. Require `abs(z) > k` and
`abs(r4) < 2*v_r`; target sign is `sign(z)`. Thus the quiet-price bound is one
square-root-of-time-scaled hourly standard deviation over four hours. No
later breakout or eventual peak is part of admission.

**Grid.** `k ∈ {2,3}` × `H ∈ {6 h,24 h}` = **4 configurations**. Four-hour premium
windows and 672 baseline observations are fixed.

**Expected failure.** Premium may reflect transitory perp demand immediately
arbitraged away, with no BTC directional information. Elevated premium can
instead precede reversal; funding payments can make the predicted direction
more expensive. Correlated overlapping windows do not create independent events.

**Data needed.** UM closes/count, premium closes/count; funding for costs only.
The earliest required boundary is `t-687 h` for the first historical premium
window. Require a complete 688 h bounded buffer, including baseline endpoint
prices. No spot, index-price, order book, or expected-funding series is assumed.

## B3-03: failed escape from a known hourly range

**Hypothesis.** An hourly excursion beyond a previously known range followed
by a close back inside can expose failed directional demand and a subsequent
reversal. This is a failed-breakout event, rather than S02/H3 continuation or
S05's distance from a moving average. It does not identify stop orders.

**Features and rule.** Decide hourly at `t + 1 ms`. Let `U` and `L` be the maximum
high and minimum low over the `W` complete hours ending at `t-1 h`, excluding
the candidate hour `[t-1 h,t)`. Let `A` be the mean true range of the preceding
24 complete hours, also excluding the candidate. Each true range is
`max(high-low, abs(high-previous_close), abs(low-previous_close))`.
Require `A > 0` and `U > L`. For candidate OHLC `O,h,l,c`, define an upper failure
as `h > U + A/4` and `L < c < U`, and a lower failure as
`l < L - A/4` and `L < c < U`. Upper only enters short; lower only enters long;
both or neither produce no entry. The decision observes the entire candidate
hour; the excursion itself is never an executable entry timestamp.

**Grid.** `W ∈ {24 h,72 h}` × `H ∈ {6 h,24 h}` = **4 configurations**. The quarter
true-range excursion and strict return inside are fixed.

**Expected failure.** A strong trend can repeatedly close inside before breaking
out for good. Hourly OHLC cannot reveal intrahour ordering or prove exhaustion.
Entry one minute later and round-trip costs may remove a visually obvious fade.

**Data needed.** UM OHLC/count and funding for costs only. Cap history at 74 hourly
bars: 72 preceding range bars, the candidate, and the earlier close needed for
true range. No liquidation, trade classification, or depth data is inferred.

## B3-04: delayed recovery after a price shock

**Hypothesis.** A large hourly displacement that stops making new extremes and
partially retraces over two more hours can continue reverting over the next
half-day/day. The hub's exhaustion counterexamples motivate requiring observed
recovery. Bars alone cannot attribute the shock to liquidation or false news.

**Features and rule.** Decide hourly at `t + 1 ms`. The shock hour is
`[t-3 h,t-2 h)` with return `r = R(t-2 h,1 h)`, high `h_s`, low `l_s`, and
`a = sign(r)`. Let `v` be `sd` of the 672 hourly returns ending at boundaries
`t-675 h, ..., t-4 h`, strictly before the shock. Require `abs(r) > k*v`.
For post-shock return `p = R(t,2 h)`, require
`-0.75*abs(r) <= a*p <= -0.25*abs(r)`.
For a positive shock, both subsequent hourly highs must be `<= h_s`; for a
negative shock, both subsequent lows must be `>= l_s`. Enter sign `-a` only
after all three hours close. Record the shock-hour key and consume it once;
accept no new shock entry until at least 24 h after the previous actual entry,
in addition to the shared flat/rearm rules.

**Grid.** `k ∈ {3,4}` × `H ∈ {12 h,24 h}` = **4 configurations**. Two-hour delay,
25–75% retracement in simple-return units, and 24 h minimum spacing are fixed.

**Expected failure.** The new price level may be justified by persistent news,
and a temporary retracement can be followed by trend resumption. Waiting two
hours can select already completed rebounds. Shock-period slippage can exceed
the bar-based cost assumption; overlapping shocks still require cluster analysis.

**Data needed.** UM OHLC/count and funding for costs only. A complete 677 h
bounded buffer covers baseline endpoint prices, shock, and confirmation. No
liquidation feed, official correction, or causally identified shock is assumed.

## B3-05: funding sign reset with price confirmation

**Hypothesis.** A funding sign change following persistent opposite-signed
settlements, accompanied by price and premium confirmation, can mark a durable
demand transition. This is a sparse continuation alternative to H4's fade; its
increment over price momentum is explicitly in doubt.

**Features and rule.** For a settlement `s`, require the immediately preceding
three rates to have the same nonzero sign `-a` and the newly admitted `f_s` to
have sign `a`. All four must be consecutive on the validated 8 h grid.
At `s + 60 min + 1 ms`, require `a*R(s+1 h,24 h) > 0`,
`a*M(s+1 h,4 h) > 0`, and `abs(f_s) > k*v_f`, where `v_f` is `sd` of the 270
settled rates strictly before `s`. Require that the new rate was admitted by
the decision; otherwise discard the event. Enter sign `a` and consume `s` once.
There is no use of the next settlement's rate, predicted rate, or final premium
average for an unfinished funding interval.

**Grid.** `k ∈ {1,2}` × `H ∈ {24 h,48 h}` = **4 configurations**. Three prior
same-signed settlements, 270-rate scale, and 1 h confirmation delay are fixed.

**Expected failure.** Sign resets can be rare, tiny, or mechanical; near-zero
scale makes the magnitude test uninformative. Price momentum may explain all
apparent benefit. Holding the newly expensive side can lose both directionally
and through funding. Report eligible-event counts before treating a tiny sample
as testable; do not loosen thresholds to obtain trades.

**Data needed.** UM closes/count, settled funding with effective availability,
and premium minute closes/count. Warmup is at least 90 d for the 270-rate scale
and 24 h for price confirmation, inside the development interval.

## Issue #24: official-correction residual reversal is contingent

The hypothesis is that some BTC price displacement caused by a false rumour
remains after an authoritative denial/correction, leaving a delayed reversal
after realistic collection and verification latency. None of B3-01–05 tests
whether a rumour was false or whether a correction caused a move.

Community Notes may supply discovery IDs/URLs for a research inventory only.
Neither note creation, eventual helpful status, a downloaded status history,
nor the timestamp of the original rumour is a live correction timestamp.
Do not backdate knowledge that a claim was false.

A future implementation needs a separately approved event input: stable event
ID, rumour source/hash and revision, authoritative correcting source/hash and
revision, source-claimed publication time, independently evidenced first-public
availability, first collector reception, and completion of source verification.
The correcting authority and eligibility rules must be frozen without looking
at subsequent prices; unresolved rumours and corrections followed by continued
price movement stay in the inventory. Preserve duplicates as one event cluster
and revised payloads as distinct versions, without overwriting earlier knowledge.

Only after that evidence exists, a possible fixed market feature is displacement
`x = C(t_d)/C(t_0)-1`, where `t_0` is the last whole-minute boundary before the
first evidenced rumour availability and `t_d` is the first whole-minute boundary
at or after correction availability, reception, and verification have all
completed. At `t_d + 1 ms`, enter `-sign(x)` only if the rumour and correction
still refer to the same claim and the direction of `x` matches the claim's
preassigned price-impact label. A possible two-point hold grid is `{1 h,4 h}`;
do not search events or timestamps using its returns. Compare with B3-04 and
matched price-shock/time-of-day controls to separate correction information
from generic recovery. If official-source PIT evidence is unavailable, classify
the event as unavailable, not as a simulated live opportunity.

Failure modes are immediate correction pricing, wrong attribution, other
simultaneous news, selected famous successes, and disappearance after reception
delay/costs. The event feed and historical first-seen evidence do not exist in
the stated free market-data inputs. This remains a data-contract investigation,
outside the five implementable market-data designs and their grid count.

## Selection, prerequisites, and verification

Suggested implementation order is **B3-03**, **B3-04**, then **B3-01**;
**B3-02/B3-05** are lower-priority continuation controls. The first two require
only the existing UM input and funding cost path. The last three require a
minute-level causal premium stream; H5's dataset sign-off does not establish
that engine interface or this note's stricter complete-window coverage.

Twenty configurations are proposed, **zero authorized or run**. Queue v2's
planned total is 199 against its 202 budget; adding all twenty would make 219,
before aborted retries. Any promotion therefore needs an explicit queue/trial
budget amendment and a frozen preregistration. This note does not amend the
queue, ledgers, holdout, gates, or existing frozen specifications.

Before a promoted family runs, retain chronological development walk-forward,
train-only parameter selection with the existing stay-flat rule, trial accounting
across all attempted families, fees/slippage/funding, boundary guards, and the
unchanged rejection gates. Count flat calendar days in return denominators;
report qualifying observations, independent event clusters, actual entries,
refused fills, forced exits, and missing-window exclusions separately. Keep a
price-only baseline for premium/funding families; every tried ablation also
consumes the research trial budget. Insufficient events are a finding, not a
reason to relax the sample-count gate.

Required future behavior evidence includes Gherkin cases for equality at each
timestamp/threshold, an unavailable constituent, tied/zero-scale baselines,
dual-sided failed excursions, ignored in-position signals, one settlement/shock
consumption, and the first possible entry open. Truncation proptests must mutate
later OHLC, premium and funding rows, including a delayed constituent and a
schema-1 premium day, without changing an earlier output. Test the explicit
buffer caps and restart/event deduplication. These are proposed acceptance
requirements, not executable specs supplied by this documentation change.

Verification for this note: TLC **not run** (local `tla2tools.jar` absent);
Kani **blocked before proofs** (`cargo kani -p parangsae-core`, verifier 0.68.0,
cannot resolve offline Cargo metadata); cucumber/proptest **not executed**
(`cargo test --workspace` blocked before tests). `spec inspect` was attempted
before writing; inspect, enforce, clippy, tests, and Kani are blocked by uncached
`camino v1.2.6`. The first inspect invocation attempted dependency downloads and
failed DNS; subsequent Cargo checks used offline mode. `cargo fmt --all -- --check`
passed. A documentation check confirmed five four-point grids, local link
targets, and whitespace. No strategy or invariant behavior changed. Diff-check
outcomes are reported with the commit handoff; CI remains required before landing.
