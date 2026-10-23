# H5 premium-index dataset sign-off — 2026-10-23

For [the frozen H5 preregistration](../prereg/2026-10-23-h5-premium-reversal.md), Data §2, Rule §3 and its contingent reader requirement. H5 consumes BTCUSDT minute premium closes as ratios: the UTC daily mean uses only `count > 0` minutes and requires at least 1,380/1,440 valid minutes per day. Its window is 2020-12-18..2025-09-30 UTC. Index price is diagnostic only; funding is a cost input.

Evidence is limited to these files; their SHA-256 hashes were verified before writing:

| File | SHA-256 |
| --- | --- |
| `/workspace/reports/research-read-path.md` | `09a1e930bd2ccd8998465df96eba378c434aa3c2d88d2940e7cbee8d9561fdac` |
| `/workspace/reports/research-read-path.sql` | `a9448cc4542116bc83f2a94f524899f9be8e1837b2cafd32cb8160c8e1da58c3` |
| `/workspace/reports/coverage-exclusions.csv` | `53076d2f1b72d14c948988176cae0ad56a467270bdbcf2e5ad33cd8927404be3` |

## Source

The read path identifies Vision `premiumIndexKlines`, BTCUSDT, one-minute bars, stored at `/workspace/data/parangsae/parquet/premium-index-kline/symbol=BTCUSDT/date=YYYY-MM-DD/part.parquet`. It documents ingestion from checksum-verified monthly archives and validation against monthly and daily archives: 35/35 checked archives passed CHECKSUM verification (30 monthly, 5 daily). Daily archives are documented as validation sources, not as ingestion sources for every stored day.

Host: the collector fetches only from `data.binance.vision` (`crates/collector/src/fetch.rs`, `VISION_HOST`, enforced by `require_public_host`). These are public archives, and no API key is used or stored.

## Scaling

Premium OHLC are stored as integer ticks; ratio = ticks / 100,000,000. `premium_1m` exposes `open_ticks/1e8 AS open`, likewise `high`, `low`, `close`, plus `count AS samples` and stored `available_ts_us`; meaningless source volume columns are omitted. The stored column `tick_domain` is `ratio` on every row read (checked by DuckDB on the parquet).

`premium_1d.mean_close` is `avg(close)` over **all present minutes**, including count-zero carry bars. H5 must instead compute its daily mean from `premium_1m.close WHERE samples > 0`, applying the preregistered 1,380/1,440 threshold. `valid_minutes` alone does not make the existing `mean_close` conform to H5.

## available_ts rule

Each minute bar is available at open + 60 seconds: the report records 0 exceptions over 3,579,639/3,579,840 present/expected premium minutes across 2020-01-01..2026-10-21. `premium_1d.available_ts_us = max(available_ts_us)` over its member minutes; on a complete day this is the next 00:00 UTC. Point-in-time admission requires `available_ts_us <= decision_ts`.

## Known gaps

The CSV contains 11/18 exclusion records for premium, totaling 201/3,579,840 expected minutes over 2020-01-01..2026-10-21. All are `known_source_absence`: absent from checksum-verified monthly archives. Premium gap absence was not checked against daily archives. Nothing is filled or interpolated.

Missing bar opens below are UTC and inclusive. Each row's denominator is the expected minutes in that UTC day; multiple rows may belong to the same day.

| Date | First–last missing open (UTC) | Missing minutes / day grid |
| --- | --- | --- |
| 2020-01-19 | 13:09–13:37 | 29/1,440 |
| 2020-12-01 | 22:40–23:59 | 80/1,440 |
| 2020-12-02 | 00:00–00:01 | 2/1,440 |
| 2020-12-17 | 07:32–07:55 | 24/1,440 |
| 2022-07-12 | 12:57–12:57 | 1/1,440 |
| 2022-07-12 | 13:14–13:21 | 8/1,440 |
| 2022-07-12 | 13:52–14:31 | 40/1,440 |
| 2022-07-13 | 06:59–06:59 | 1/1,440 |
| 2023-11-10 | 03:38–03:41 | 4/1,440 |
| 2023-11-10 | 03:59–04:08 | 10/1,440 |
| 2024-08-12 | 10:02–10:03 | 2/1,440 |

Within H5's window, 2,517,054/2,517,120 expected minutes are present; 66/2,517,120 are absent. The incomplete days are 2022-07-12, 2022-07-13, 2023-11-10 and 2024-08-12 (4/1,748 window days). The read-path contract says to score days with exclusions as excluded.

On 2023-08-29, 86/1,440 daily bars in the 14:33–15:59 UTC range are count-zero flat carry-forwards; valid minutes are 1,354/1,440. Both checksum-verified monthly and daily archives contain those bars. This is a source property, with no parse discrepancy established by the checks; it is the only window day below the 1,380/1,440 valid-minute rule (1/1,748 days).

The source also contains 3,633/3,579,639 stored premium bars with all-zero OHLC, all in 2020-01..2020-11; 0/2,517,054 present H5-window bars have this property. Treat them as missing, not zero premium. They are outside H5's window.

## Schema-1

Headerless Vision CSVs account for 824/2,486 stored premium days, spanning 2020-01..2022-05; 472/1,748 H5-window days use schema-1. A fresh decimal parse matches stored open, high, low, close, count and close_time on 1,186,425/1,186,425 schema-1 rows across 824/824 schema-1 days, and 128,160/128,160 schema-2 control rows across the reported 89 control days.

Schema-1 and schema-2 differ only in the raw CSV; the normalizer writes one parquet schema (19 columns, including `schema_version`). A single non-union-by-name read over 2,486/2,486 stored premium day files succeeds, so any reader of `parquet/premium-index-kline` or `premium_1m` reads schema-1 days identically to schema-2 days. Caveat: engine-v1 has no premium stream yet (prereg, Open items); when one is built, its truncation proptest should include at least one schema-1 day.

## Holdout

The SQL bounds row views to open/funding time < 2025-10-01 00:00 UTC unless explicitly opted in with `SET VARIABLE readpath_include_holdout = true;`. Metadata coverage views are unbounded and carry no values. The runner's partition guard remains the primary control; H5 must also enforce its lower window bound.

The holdout bound and the `samples`/`valid_minutes` columns landed in read-path commit `6b7119a`, which is an ancestor of this branch. The hashes above pin the exact files; they supersede the preregistration's older read-path pins.

## Sign-off

**Data Engineer, 2026-10-23:** signed off on the documented dataset identity, ratio conversion, availability rule, source gaps and decimal-parse agreement within the evidence above. This covers all five contingent items: source, `tick_domain = 'ratio'` 1e8 scaling, the `available_ts` rule, gaps as listed, and schema-1 readability. Conditions: H5 must use the filtered daily mean specified above, exclude 2023-08-29 and the coverage-excluded days, and the future engine-v1 premium stream must carry a schema-1 day in its test. This is a dataset evidence sign-off, not a strategy result or readiness verdict.

Verification: hashes matched; CSV timestamps, inclusive minute lengths, premium record count and window totals checked. TLC, Kani and cucumber were not run for this documentation-only change. `cargo xtask spec inspect` could not run: `cargo_metadata` was unavailable locally and crates.io DNS resolution failed.
