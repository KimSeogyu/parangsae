# Vision fixtures

Small slices used by the collector checks. They are not a market-data archive.

CSV files are unmodified line ranges from public `data.binance.vision` objects, plus rows edited only where the name says `bad`, `missing`, `off`, or `duplicate`. The edited cells are the ones the scenario is about.

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
