# 18-family offline research catalogue

Source: Henry, **퀀트가 돈을 버는 18가지 트레이딩 전략들**,
[2026-11-01 Naver article](https://m.blog.naver.com/leebisu/224461004120).
The headings below preserve the source order. Mechanisms are deliberately narrow
engineering baselines, not replications of the article's performance claims or
cited papers. The article supplied no executable specification. No alpha,
profitability, current-market availability or readiness conclusion is made.

## Entry point and scope

`parangsae_backtest::catalog::evaluate(InputUniverse, Request)` is a **pure,
in-memory dispatcher**, despite its containing crate's name. It does not call
`run`, read a tape, create a trial, generate returns, place orders or mint RiskGate
authority. `Request` identifies the family through its typed variant; its output
retains the difference between feature, model, allocation and execution plan.

- `InputUniverse::BtcUsdM` allows eight compatible single-instrument research
  families; it refuses ten requiring unavailable cross-asset/spot/options/event/
  top-of-book domains. This is a compatibility gate, **not** a data-admission gate.
- `InputUniverse::SuppliedOffline` accepts all eighteen **when the caller supplies
  the required typed causal inputs**. This does not assert that those data exist
  historically, are point-in-time correct, or have a supported market adapter.
- Neither mode is connected to the existing historical study runners. A future
  study needs separately reviewed sources, protocol, costs and trial budget.
- Existing S01–S06/H1–H5/C1 identities, configurations, preregistrations, historical
  results and holdout controls are not repurposed by this catalogue.

## Source-to-code coverage

`C` means the current BTC USD-M domain accepts the typed research request;
`S` means supplied-offline only. Both still require explicit caller inputs.

| # / source heading | Implemented baseline and output | Domain / additional requirements |
| --- | --- | --- |
| 1 모멘텀 (Momentum) | `PriceRule::Momentum`: trailing return threshold → direction | C; closed bars, explicit lookback/threshold |
| 2 평균회귀 (Mean Reversion) | `PriceRule::MeanReversion`: deviation from prior mean, excluding current → opposite direction | C; closed bars |
| 3 통계적 차익거래 (Statistical Arbitrage) | `models::statistical_arbitrage`: supplied past-fit two-factor residuals → most under/over-valued symbols | S; synchronized unique-symbol panel, coefficients fitted before snapshot; not a cointegration or factor-neutral hedge proof |
| 4 페어 트레이딩 (Pairs Trading) | `panel::pairs`: fixed-beta spread versus its prior mean → two sides and hedge ratio | S; two synchronized instruments, positive preselected beta; no beta fitting or convergence guarantee |
| 5 추세 추종 (Trend Following) | `PriceRule::TrendFollowing`: fast/slow moving-average comparison | C; distinct from trailing-return momentum but overlapping exposure |
| 6 시장조성 (Market Making) | `planning::quotes`: one-tick-wider noncrossing quotes with side-specific inventory limits | S; fresh top of book, tick/lot units and inventory; no queue/fill/adverse-selection model |
| 7 이벤트 드리븐 (Event-Driven) | `models::event_driven`: newly visible actual-minus-prior-expectation surprise | S; explicit announcement/forecast clocks and economic sign encoding; no event scraping |
| 8 팩터 투자 (Factor Investing) | `panel::factor`: fixed four-characteristic weighted score → equal-gross long/short tails | S; prior point-in-time characteristics and caller-selected weights; no characteristic normalization hidden |
| 9 캐리 트레이드 (Carry Trade) | `derivatives::carry`: same-horizon income minus financing and explicit costs | S; common-horizon terms; no funding forecasts, capital custody or borrow availability assumed |
| 10 변동성 차익거래 (Volatility Arbitrage) | `derivatives::volatility_arbitrage`: forecast-minus-implied variance after buffer → option side plus delta-hedging intent | S; matched variance horizon and option data; no Greeks, dynamic hedge or listed-option adapter |
| 11 옵션 전략 (Option Strategies) | `derivatives::option_payoff`: covered-call/cash-secured-put expiry scenario and required collateral | S; one whole unit, European expiry assumption; no actual collateral reservation, multiplier or expiry authentication |
| 12 횡단면 알파 (Cross-Sectional Alpha) | `panel::rank`: supplied score ranks → balanced tails | S; current synchronized panel; boundary score ties refused rather than arbitrary alpha |
| 13 단기 반전 (Short-Term Reversal) | `PriceRule::ShortTermReversal`: large latest one-bar move with elevated volume versus the prior lookback baseline → fade | C; volume-carrying closed bars and explicit thresholds |
| 14 돌파 매매 (Breakout Trading) | `PriceRule::Breakout`: current close beyond prior highs/lows, excluding candidate | C; strict comparison, equality neutral |
| 15 리스크 패리티 (Risk Parity) | `allocation::inverse_volatility`: bounded positive quantized weights summing exactly to one | S; **diagonal/zero-correlation assumption only**, shared forecast horizon; not general correlated ERC |
| 16 국면 전환 (Regime Switching) | `VolatilityRegime`: explicit low/high hysteresis → normal/defensive budget proposal | C; new causal volatility estimates; not learned latent-state inference or RiskGate permission |
| 17 머신러닝 알파 (Machine Learning Alpha) | `RidgeModel::fit` + `predict`: actual bounded univariate ridge/intercept fitting, then later predictor | C; causal labeled training window, strict label cutoff and later prediction; no tuning or accuracy claim |
| 18 집행 알고리즘 (Execution Algorithms) | `planning::twap`/`vwap`: deterministic lot-conserving time/volume-weighted schedules | C; positive integer lots, bounded horizon, prior historical/forecast volume profile; no venue orders, adaptive impact or future realized volumes |

## Contracts and limitations

Price inputs use canonical closed H1/H4 bars; windows are bounded and the prior
baseline excludes the candidate where required. Neutral is distinct from missing
history, gap or stale data. Public causal views are rechecked at the supplied
decision; possession of a later view cannot authorize an earlier decision.
Panel ordering, duplicate keys, ties, synchronized clocks and overflow have
explicit refusal paths. Model labels must be available before training and
prediction must be strictly later; a timestamp field itself is not provenance.

Price and quantity arithmetic is checked/integer in money paths. Option payoffs
state required—not reserved—collateral. Risk parity uses reciprocal precision
1e12 and weight precision 1e8 with deterministic largest-remainder allocation;
quantization means approximate equal risk even under its diagonal model. If the
1e8 weight grid would round any admitted asset to zero, `Resolution` is returned
instead of silently removing it.
TWAP/VWAP allocate integer **venue lots**, not base quantity ticks, and refuse
zero-lot slices. Quote price ticks and inventory lot units must match caller
instrument metadata. These plans cannot submit or execute an order.

No costless outcome is a return estimate. Carry/option scenarios include their
explicit input costs; all other signals/plans require later fee, spread,
slippage, funding, borrow, financing, impact and capital models before economic
evaluation. None is silently advertised as an implementable profitable trade.

## Existing correctness repairs

Two actual synthetic regressions failed before the repair:

1. A rejected zero-price S05 bar consumed `Cursor.last_open`; a corrected bar at
   the same time returned `Gap`. The shared cursor now validates positive,
   exactly representable OHLC before committing the key. This intentionally
   tightens the executable bar domain for all legacy families, including ones
   whose immediate arithmetic is integer-only.
2. S01 consumed a valid signal bar before discovering an invalid decision book.
   TSMOM and ensemble now prepare the book's checked signed weight before any
   history mutation. Invalid supplied books are rejected even during warmup,
   rather than silently ignored until a strategy emits.

Regression observers compare corrected outputs and subsequent valid suffixes
with fresh instances. This is a targeted refusal-state repair, not a claim that
all existing algorithms or economics have been validated.

## Reproduce without market data

```sh
cargo test -p parangsae-strategies --lib catalog
cargo test -p parangsae-strategies --test refused_bar_retry
cargo test -p parangsae-risk --lib allocation
cargo test -p parangsae-execution --lib planning
cargo test -p parangsae-backtest --test catalog_price_panel
cargo test -p parangsae-backtest --test catalog_models
cargo test -p parangsae-backtest --test catalog_operations
cargo run -p parangsae-backtest --example catalog18 -- list
cargo run -p parangsae-backtest --example catalog18 -- twap
```

The three Gherkin callers invoke the actual catalogue dispatcher, assert exact
synthetic outputs/refusals, and fail on skipped steps. The `catalog18` example
lists capabilities or prints a synthetic lot schedule; it reads no data.
Green tests establish these contracts, not trading edge. TLC/Kani results remain
separate checks; no new formal proof of the statistical models is claimed.
