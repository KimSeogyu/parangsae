# Integrity fixtures

Verbatim line ranges from public `data.binance.vision` objects. The fixture bytes are the CSV member, not the zip, so a test that needs archive identity injects the path and SHA-256 below and does not hash the fixture.

### A1 `um_monthly_2023-11-10_slice.csv`

Source: data/futures/um/monthly/klines/BTCUSDT/1m/BTCUSDT-1m-2023-11.zip, SHA-256 63aa156170b2316927ae4c4928cc78866934a02b4bbc14c730b5fe4790383a80, Last-Modified Mon, 08 Jan 2024 08:33:32 GMT. 100/103 rows differ from A2: keys 1699628760000..1699634700000.

### A2 `um_daily_2023-11-10_slice.csv`

Source: data/futures/um/daily/klines/BTCUSDT/1m/BTCUSDT-1m-2023-11-10.zip, SHA-256 60fbc08c0a73afa5f04d8bc9b8b960cb782435147c1a1080a2b449714db1ea20, Last-Modified Mon, 13 Nov 2023 07:51:38 GMT.

### B1 `um_monthly_2023-11-14_slice.csv`

Source: data/futures/um/monthly/klines/BTCUSDT/1m/BTCUSDT-1m-2023-11.zip, SHA-256 63aa156170b2316927ae4c4928cc78866934a02b4bbc14c730b5fe4790383a80, Last-Modified Mon, 08 Jan 2024 08:33:32 GMT. 5/7 rows differ from B2: keys 1699961880000..1699962120000.

### B2 `um_daily_2023-11-14_slice.csv`

Source: data/futures/um/daily/klines/BTCUSDT/1m/BTCUSDT-1m-2023-11-14.zip, SHA-256 8c2dbbf36dfa6ca479e7125c90ec26488b6d402446bdd823df819d80bb325be5, Last-Modified Wed, 15 Nov 2023 06:56:32 GMT.

### D1 `BTCUSDT-fundingRate-2020-01.vision.csv`

Source: data/futures/um/monthly/fundingRate/BTCUSDT/BTCUSDT-fundingRate-2020-01.zip, SHA-256 7f81b2f3694d13779e7e896b69d60cd61e9444d7b9f9e90df761935e1c1b76e2. Full month, 93 rows, verbatim. This is not `fixtures/vision/BTCUSDT-fundingRate-2020-01.csv`, whose lines 2–10 were edited.
