# parangsae research read path v0.4 (BTCUSDT USD-M) — 2026-10-23

Research window starts 2020-01-01 (pre-2020 is out of scope).

One way to load: DuckDB views in `/workspace/reports/research-read-path.sql`
(`duckdb -c ".read /workspace/reports/research-read-path.sql" -c "select ... from kline_1h"`).
Rust/Polars/Arrow readers: same globs, plain Parquet, Hive partitions. Store is read-only for consumers.

## Layout
`/workspace/data/parangsae/parquet/<dataset>/symbol=BTCUSDT/date=YYYY-MM-DD/part.parquet` (one file per UTC day)
- 1m klines: `um-kline`
- funding: `/workspace/data/parangsae/runs/funding-4db8a75/parquet/funding-rate/...` (fresh main 4db8a75 ingest; the old store copy is identical on rate and calc_time, 7,395/7,395)
- also present (not part of v0 contract): mark-price-kline, index-price-kline, premium-index-kline, spot-kline, metrics, book-depth

## Units
- Times: UTC microseconds (`*_us`). Fixed point: `*_ticks / 1e8` (price, volume, rate).
- Point-in-time key: `available_ts_us`. Never join on a value before its `available_ts_us`.
  - 1m kline: available = open_time + 60 s on 3,581,280/3,581,280 rows (measured). 1h derived: max over its 60 minutes (never before the hour ends).
  - funding view: available = greatest(stored, slot + 60 s), which gives the same result before and after C7. Rust readers must use this same expression.
  - funding: main does not implement available_ts_rules/v0.3 yet (unit C7). Stored available = funding_time, earlier than event_ts on 3,312/7,395 rows. The `funding` view applies slot + 60 s. Do not read the raw column until C7 lands.

## Coverage (as of 2026-10-23 07:12 KST)
| dataset | range | days | rows | notes |
|---|---|---|---|---|
| 1m klines | 2019-12-31..2026-10-21 | 2,487/2,487 | 3,581,280 (1,440/day on every day) | |
| 1h klines (derived) | 2019-12-31..2026-10-21 | 2,487/2,487 | 59,688 (24/day) | 744/744 hours exact vs Vision 1h 2024-01 |
| funding (8h) | 2020-01-01..2026-09-30 | 2,465/2,465 | 7,395 (3/day) | 2026-10 monthly archive not yet published |

Out of scope: pre-2020. October 2026 funding lands with the Nov monthly archive.

## Resolved conflicts (v0.3)
`kline_1m`/`kline_1h` read `um_1m_resolved`. The 105 real-value um-kline conflicts are all resolved, so no forced-flat window is needed:
- 2023-11-10 15:06–16:45 UTC (100 min): the stored monthly rows are filler (99/100 with count 0). They are replaced by the daily archive (sha 60fbc08c…) via `/workspace/data/parangsae/runs/resolutions/um-kline-2023-11-10.parquet` (sha d5f6bcd6…). The aggTrades check gives 79/100 exact and 100/100 within 0.50 USDT; signed by Reviewer 2026-10-22.
- 2023-11-14 11:38–11:42 UTC (5 min): the stored monthly rows are correct (5/5 exact vs aggTrades), so no change.

## Mark price 1m (`mark_1m`, added v0.2 for engine-v1)
Source `parquet/mark-price-kline`, 2020-01-01..2026-10-21, 3,579,785/3,579,840 minutes, complete on 2,483/2,486 days. Availability is open + 60 s on every row.
Missing bar open times (UTC, inclusive), which are real source gaps (`mark_1m_gaps`):
| first missing open | last missing open | minutes |
|---|---|---|
| 2020-01-19 13:09 | 2020-01-19 13:37 | 29 |
| 2020-12-17 07:32 | 2020-12-17 07:55 | 24 |
| 2024-08-12 10:02 | 2024-08-12 10:03 | 2 |
`day_completeness` gives minutes per day for kline_1m and mark_1m.

## Premium index and index price 1m (`premium_1m`, `index_1m`, `premium_1d`, added v0.4 for H5)
Sources are `parquet/premium-index-kline` and `parquet/index-price-kline`, ingested from the checksum-verified Vision monthly archives. Both cover 2020-01-01..2026-10-21, with a grid of 3,579,840 minutes over 2,486 days.
- `premium_1m` has 3,579,639/3,579,840 minutes (201 missing in 11 gap records) and is complete on 2,478/2,486 days.
- `index_1m` has 3,579,780/3,579,840 minutes (60 missing in 4 gap records) and is complete on 2,482/2,486 days.
- Units: `*_ticks / 1e8`. The premium is a ratio, so 0.0009 means 0.09 %. Availability is open + 60 s on every row (0 exceptions in either view). The source volume and trade columns carry no meaning here and are omitted.
- `premium_1d` has one row per UTC day and every day is emitted. Its columns are `day_open_us, date, open, high, low, close` (the last present 1m close), `mean_close` (the mean of present 1m closes), `minutes`, `complete` (1440/1440) and `available_ts_us` (the max over members, which is the next day's 00:00 UTC on complete days). Use a row only when available_ts_us ≤ decision_ts.
- For 2020-12-18..2025-09-30 (2,517,120 minutes, 1,748 days):
  - premium has 2,517,054/2,517,120 minutes, with 4 incomplete days: 2022-07-12, 2022-07-13, 2023-11-10 and 2024-08-12.
  - index has 2,517,113/2,517,120 minutes, with 2 incomplete days: 2022-07-12 and 2024-08-12.
- Source note: 3,633 premium bars are all zero (OHLC = 0), all of them in 2020-01..2020-11 and none in 2020-12-18..2025-09-30. They are present in the Vision source as published; for example, 2020-01 has 7/7 matching rows with the raw CSV. Treat them as missing values, not as a premium of 0.

## Coverage exclusions (`coverage_exclusions`, `coverage_excluded_days`, `coverage-exclusions.csv`, v0.4)
- There is one row per open real source gap in mark, index and premium 1m: 18 records (mark 3, index 4, premium 11) with 316 missing minutes. kline_1m has none.
- Columns: `dataset, view_name, date, first_missing_open_us, last_missing_open_us, missing_minutes, source_status`. Missing bar opens are UTC and inclusive.
- `source_status` is `known_source_absence` on every row: the bars are absent from the checksum-verified Vision monthly archive. The daily archive was checked only for mark 2024-08-12, where the bars are also absent.
- Gaps stay gaps: nothing is filled or interpolated.
- `coverage_excluded_days` gives `(view_name, date)` for every UTC day touched by a gap. Totals are mark 3 days, index 4 days and premium 8 days.
- Within 2020-12-18..2025-09-30 it covers mark 1 day, index 2 days and premium 4 days. A consumer should score a day with an exclusion as excluded instead of aborting.
- `coverage-exclusions.csv` next to the SQL is the same table, exported for readers without DuckDB.

## Gaps
Ledger: `/workspace/data/parangsae/runs/gap-resolutions-2026-10-23.csv` (gap records in the store are not modified).
- resolved: 127/127 `missing_file` records on 1m klines (um 10, mark 54, index 59, premium 4). The daily archive was missing, but every slot is stored from the monthly archive.
- open, real source gaps: 26 `grid` records (mark 3, index 4, premium 11, spot 8), none in um-kline. Funding has one open record, 2026-10-01 `missing_file`, until the Nov archive.

## Raw schema (um-kline part.parquet)
symbol, interval_us, open_time_us, close_time_us, open/high/low/close_ticks, volume_ticks, quote_volume_ticks, count,
taker_buy_volume_ticks, taker_buy_quote_volume_ticks, event_ts_us, available_ts_us, received_ts_us, schema_version, tick_domain, date
## Raw schema (funding-rate part.parquet)
symbol, raw_calc_time_us, funding_time_us, interval_hours, rate_ticks, event_ts_us, available_ts_us, received_ts_us, schema_version, date
