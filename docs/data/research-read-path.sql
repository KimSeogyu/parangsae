-- parangsae research read path v0.4 (DuckDB). Run: duckdb -c ".read /workspace/reports/research-read-path.sql" ...
-- Store root: /workspace/data/parangsae/parquet  (read-only for consumers)
-- Fixed point: *_ticks / 1e8. Times: UTC microseconds. Point-in-time key: available_ts_us.
-- Holdout bound (v0.4, defense in depth): every row view below returns only rows with event time
-- < 2025-10-01 00:00 UTC (1759276800000000 us) unless the session explicitly opts in with
--   SET VARIABLE readpath_include_holdout = true;
-- The runner's partition guard stays the primary control; this only stops an accidental glob read.
-- Metadata views (day_completeness, *_gaps, coverage_*) are not bounded and carry no values.
-- um-kline 1m with reviewed resolutions applied (store unmodified).
-- 2023-11-10 15:06..16:45 UTC (100 min): the stored monthly rows are synthetic filler (99 zero-count bars);
-- they are replaced by the daily archive rows (aggTrades arbitration, Reviewer-signed). 2023-11-14 11:38..11:42 (5 min): stored monthly is correct.
create or replace view um_1m_resolved as
select * from read_parquet('/workspace/data/parangsae/parquet/um-kline/symbol=BTCUSDT/*/part.parquet') s
where s.open_time_us not in (select open_time_us from read_parquet('/workspace/data/parangsae/runs/resolutions/um-kline-2023-11-10.parquet'))
union all by name
select * exclude (resolution) from read_parquet('/workspace/data/parangsae/runs/resolutions/um-kline-2023-11-10.parquet');

create or replace view kline_1m as
select open_time_us, close_time_us,
       open_ticks/1e8 as open, high_ticks/1e8 as high, low_ticks/1e8 as low, close_ticks/1e8 as close,
       volume_ticks/1e8 as volume, quote_volume_ticks/1e8 as quote_volume, count as trades,
       taker_buy_volume_ticks/1e8 as taker_buy_volume, taker_buy_quote_volume_ticks/1e8 as taker_buy_quote_volume,
       available_ts_us
from um_1m_resolved
where open_time_us < 1759276800000000 or coalesce(getvariable('readpath_include_holdout'), false);

-- 1h derived from 1m; only complete hours (60/60 minutes). Verified 744/744 exact vs Vision 1h 2024-01.
create or replace view kline_1h as
select open_time_us//3600000000*3600000000 as open_time_us,
       open_time_us//3600000000*3600000000 + 3599999999 as close_time_us,
       arg_min(open_ticks, open_time_us)/1e8 as open, max(high_ticks)/1e8 as high,
       min(low_ticks)/1e8 as low, arg_max(close_ticks, open_time_us)/1e8 as close,
       sum(volume_ticks)/1e8 as volume, sum(quote_volume_ticks)/1e8 as quote_volume, sum(count) as trades,
       sum(taker_buy_volume_ticks)/1e8 as taker_buy_volume, sum(taker_buy_quote_volume_ticks)/1e8 as taker_buy_quote_volume,
       max(available_ts_us) as available_ts_us
from um_1m_resolved
where open_time_us < 1759276800000000 or coalesce(getvariable('readpath_include_holdout'), false)
group by 1 having count(*) = 60;

-- Funding: fresh ingest with main 4db8a75 (verify 81/81 archives, 0 faults).
-- main still stamps available_ts = funding_time and event_ts = raw calc_time (0..N ms after the slot),
-- so stored available_ts < event_ts on 3,312/7,395 rows. available_ts_rules/v0.3 (slot + 60 s) is unit C7,
-- not yet merged. Until C7 lands and funding is re-ingested, this view applies v0.3 explicitly.
create or replace view funding as
select funding_time_us, interval_hours, rate_ticks/1e8 as rate,
       greatest(available_ts_us, funding_time_us + 60000000) as available_ts_us  -- idempotent across C7
from read_parquet('/workspace/data/parangsae/runs/funding-4db8a75/parquet/funding-rate/symbol=BTCUSDT/*/part.parquet')
where funding_time_us < 1759276800000000 or coalesce(getvariable('readpath_include_holdout'), false);

-- Mark price 1m (for funding notional, equity marks, ruin check). Same units as kline_1m.
-- NOT complete on 3/2,486 days: 55 missing minutes, real source gaps (see mark_1m_gaps).
create or replace view mark_1m as
select open_time_us, close_time_us,
       open_ticks/1e8 as open, high_ticks/1e8 as high, low_ticks/1e8 as low, close_ticks/1e8 as close,
       available_ts_us
from read_parquet('/workspace/data/parangsae/parquet/mark-price-kline/symbol=BTCUSDT/*/part.parquet')
where open_time_us < 1759276800000000 or coalesce(getvariable('readpath_include_holdout'), false);

-- Per-day completeness: complete = 1440/1440 minutes present.
create or replace view day_completeness as
select 'kline_1m' as dataset, date, count(*) as minutes, count(*) = 1440 as complete
from read_parquet('/workspace/data/parangsae/parquet/um-kline/symbol=BTCUSDT/*/part.parquet', hive_partitioning=1) group by date
union all
select 'mark_1m', date, count(*), count(*) = 1440
from read_parquet('/workspace/data/parangsae/parquet/mark-price-kline/symbol=BTCUSDT/*/part.parquet', hive_partitioning=1) group by date;

-- Mark gap windows: missing bar open times first_missing_open_us..=last_missing_open_us (UTC).
create or replace view mark_1m_gaps as
select start_us as first_missing_open_us, end_us - 60000000 as last_missing_open_us,
       (end_us - start_us) // 60000000 as missing_minutes, detail
from read_parquet('/workspace/data/parangsae/gaps/mark-price-kline/symbol=BTCUSDT/*/part.parquet', hive_partitioning=1)
where kind = 'grid';

-- v0.4: premium index and index price 1m (raw Vision premiumIndexKlines / indexPriceKlines, monthly, checksums verified).
-- Same units and availability as kline_1m: *_ticks / 1e8, available_ts_us = open + 60 s on every row.
-- premium = premium index ratio (e.g. 0.0009 = 0.09 %). Volume columns are not meaningful for these sources and are omitted; premium_1m keeps the source count as `samples`.
create or replace view premium_1m as
select open_time_us, close_time_us,
       open_ticks/1e8 as open, high_ticks/1e8 as high, low_ticks/1e8 as low, close_ticks/1e8 as close,
       count as samples,  -- source sample count; 0 = no premium sample that minute (flat carry-forward bar)
       available_ts_us
from read_parquet('/workspace/data/parangsae/parquet/premium-index-kline/symbol=BTCUSDT/*/part.parquet')
where open_time_us < 1759276800000000 or coalesce(getvariable('readpath_include_holdout'), false);

create or replace view index_1m as
select open_time_us, close_time_us,
       open_ticks/1e8 as open, high_ticks/1e8 as high, low_ticks/1e8 as low, close_ticks/1e8 as close,
       available_ts_us
from read_parquet('/workspace/data/parangsae/parquet/index-price-kline/symbol=BTCUSDT/*/part.parquet')
where open_time_us < 1759276800000000 or coalesce(getvariable('readpath_include_holdout'), false);

-- Daily premium per UTC day, from premium_1m. Every day is emitted; `complete` = 1440/1440 minutes.
-- close = last present 1m close; mean_close = mean of present 1m closes.
-- available_ts_us = max member available (= next 00:00 UTC on complete days). Use only rows with available_ts_us <= decision_ts.
-- Days with complete = false are listed in coverage_exclusions; score them as excluded, do not abort.
create or replace view premium_1d as
select open_time_us//86400000000*86400000000 as day_open_us,
       to_timestamp(open_time_us//86400000000*86400)::date as date,
       arg_min(open, open_time_us) as open, max(high) as high, min(low) as low,
       arg_max(close, open_time_us) as close, avg(close) as mean_close,
       count(*) as minutes, count(*) = 1440 as complete,
       count(*) filter (where samples > 0) as valid_minutes,  -- minutes with >= 1 source sample
       max(available_ts_us) as available_ts_us
from premium_1m group by 1, 2;

-- Coverage exclusion list (machine-readable; also exported as coverage-exclusions.csv).
-- One row per open real source gap (store gap records, kind = grid) in mark/index/premium 1m.
-- um-kline (kline_1m) has none. Missing bar opens are first_missing_open_us..=last_missing_open_us (UTC).
-- source_status: known_source_absence = absent from the checksum-verified Vision monthly archive;
-- daily archive checked only for mark 2024-08-12 (also absent). Gaps stay gaps: nothing is filled or interpolated.
create or replace view coverage_exclusions as
select dataset, view_name, date, first_missing_open_us, last_missing_open_us, missing_minutes, source_status
from (
  select 'mark-price-kline' as dataset, 'mark_1m' as view_name, * from read_parquet('/workspace/data/parangsae/gaps/mark-price-kline/symbol=BTCUSDT/*/part.parquet', hive_partitioning=1)
  union all by name
  select 'index-price-kline' as dataset, 'index_1m' as view_name, * from read_parquet('/workspace/data/parangsae/gaps/index-price-kline/symbol=BTCUSDT/*/part.parquet', hive_partitioning=1)
  union all by name
  select 'premium-index-kline' as dataset, 'premium_1m' as view_name, * from read_parquet('/workspace/data/parangsae/gaps/premium-index-kline/symbol=BTCUSDT/*/part.parquet', hive_partitioning=1)
) g
cross join lateral (select g.start_us as first_missing_open_us, g.end_us - 60000000 as last_missing_open_us,
                           (g.end_us - g.start_us)//60000000 as missing_minutes,
                           'known_source_absence' as source_status)
where g.kind = 'grid'
order by first_missing_open_us, dataset;

-- Excluded UTC days per view (union of all gap windows touching the day).
create or replace view coverage_excluded_days as
select distinct view_name, d::date as date
from coverage_exclusions,
     unnest(range(first_missing_open_us//86400000000, last_missing_open_us//86400000000 + 1)) t(dn),
     (select to_timestamp(dn*86400)::date) x(d)
order by 1, 2;
