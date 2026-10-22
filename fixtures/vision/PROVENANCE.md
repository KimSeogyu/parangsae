# Vision fixtures

Small slices used by the collector checks. They are not a market-data archive.

CSV files are unmodified line ranges from public `data.binance.vision` objects, plus rows edited only where the name says `bad`, `missing`, `off`, or `duplicate`, and lines 2–10 of `BTCUSDT-fundingRate-2020-01.csv`. The edited cells are the ones the scenario is about.

`BTCUSDT-fundingRate-2020-01.csv` is not the verbatim January 2020 funding archive. Lines 2–10 (the first nine data rows) were edited to carry the scenario tokens (`-3.5E-7`, `-1E-8`, `-1.0E-7`, `6E-8`, `0E-16`, and the following rates and calc times on those lines). The real archive has `-0.00012359` at calc_time `1577836800000`. The scenario ticks from those lines are synthetic except the `8.4E-7` row, which is 84 ticks. The unmodified month is `integrity/BTCUSDT-fundingRate-2020-01.vision.csv`.

`um_kline_bad_taker_quote.csv` is the first line of `um_kline_2021-06-01_headerless.csv` with `taker_buy_quote_volume` set to `99999999.00000000`, above `quote_volume`. The other cells are unchanged.

`um_kline_slice.zip` is a zip of `um_kline_2021-06-01_headerless.csv` packed in this repo. It is not the Vision zip, and its sha256 is not the Vision digest. The Vision digest for `BTCUSDT-1m-2021-06-01.zip` observed while cutting the slice was `fc949a2cdb72753faafbed1eb93a3a1847b3ba77056b6360a3a87e93a9f37141`.

`listing_with_part.xml` uses the real daily kline key and size `67872` for `BTCUSDT-1m-2021-06-01.zip`. The `part-00000-example-c000.zip` key is synthetic. It exists so the filename allow-list has a name to quarantine.

Source objects, all under `https://data.binance.vision/`:

- `data/futures/um/daily/klines/BTCUSDT/1m/BTCUSDT-1m-2021-06-01.zip`
- `data/futures/um/daily/klines/BTCUSDT/1m/BTCUSDT-1m-2026-10-20.zip`
- `data/spot/daily/klines/BTCUSDT/1m/BTCUSDT-1m-2024-12-31.zip`
- `data/spot/daily/klines/BTCUSDT/1m/BTCUSDT-1m-2025-01-01.zip`
- `data/futures/um/monthly/fundingRate/BTCUSDT/BTCUSDT-fundingRate-2026-09.zip`
- `data/futures/um/daily/metrics/BTCUSDT/BTCUSDT-metrics-2026-10-20.zip`
- `data/futures/um/daily/metrics/BTCUSDT/BTCUSDT-metrics-2020-09-01.zip`
- `data/futures/um/daily/bookDepth/BTCUSDT/BTCUSDT-bookDepth-2026-10-20.zip`
- `data/futures/um/daily/bookDepth/BTCUSDT/BTCUSDT-bookDepth-2023-01-01.zip`

`book_gap_real_span.csv` keeps the real `2026-10-20 00:00:01` snapshot and a real later snapshot moved to `00:02:01` so the span is 120 seconds. The intermediate snapshots from that archive are omitted.
