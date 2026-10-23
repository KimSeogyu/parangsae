-- parangsae research read path v0 (DuckDB). Run: duckdb -c ".read /workspace/reports/research-read-path.sql" ...
-- Store root: /workspace/data/parangsae/parquet  (read-only for consumers)
-- Fixed point: *_ticks / 1e8. Times: UTC microseconds. Point-in-time key: available_ts_us.
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
from um_1m_resolved;

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
group by 1 having count(*) = 60;

-- Funding: fresh ingest with main 4db8a75 (verify 81/81 archives, 0 faults).
-- main still stamps available_ts = funding_time and event_ts = raw calc_time (0..N ms after the slot),
-- so stored available_ts < event_ts on 3,312/7,395 rows. available_ts_rules/v0.3 (slot + 60 s) is unit C7,
-- not yet merged. Until C7 lands and funding is re-ingested, this view applies v0.3 explicitly.
create or replace view funding as
select funding_time_us, interval_hours, rate_ticks/1e8 as rate,
       greatest(available_ts_us, funding_time_us + 60000000) as available_ts_us  -- idempotent across C7
from read_parquet('/workspace/data/parangsae/runs/funding-4db8a75/parquet/funding-rate/symbol=BTCUSDT/*/part.parquet');

-- Mark price 1m (for funding notional, equity marks, ruin check). Same units as kline_1m.
-- NOT complete on 3/2,486 days: 55 missing minutes, real source gaps (see mark_1m_gaps).
create or replace view mark_1m as
select open_time_us, close_time_us,
       open_ticks/1e8 as open, high_ticks/1e8 as high, low_ticks/1e8 as low, close_ticks/1e8 as close,
       available_ts_us
from read_parquet('/workspace/data/parangsae/parquet/mark-price-kline/symbol=BTCUSDT/*/part.parquet');

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
