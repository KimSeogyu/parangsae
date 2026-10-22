# parangsae

parangsae is a from-scratch Rust research stack for Binance USD-M BTCUSDT. Holds are mid-frequency: tens of minutes to days, on bars from 1 minute to 4 hours.

Formal specs are TLA+, checked by TLC. Behavior specs are Gherkin, checked by cucumber-rs. Live trading is not implemented. `crates/execution` has no order client.

The previous Python implementation is at the `python-legacy` tag (commit `8713f024afa18bcb9fd8e7cae573acffab6bdb7e`). Do not run it and do not port it.

## Layout

- `crates/core` — `Price`, `Qty`, `Symbol`, the three timestamps, the `Causal` gate, kline time checks
- `crates/collector` — Binance Vision backfill (`parangsae-collect`). Public `data.binance.vision` objects only. No API key, no REST market client, no WebSocket, no order route.
- `crates/signals`, `crates/regime`, `crates/execution` — skeletons
- `specs/tla/IngestSegment.tla` — ingest gap / duplicate model
- `specs/tla/ManifestWrite.tla` — idempotent fact write
- `specs/features/kline_integrity.feature` — kline clock and grid
- `specs/features/collector/vision_integrity.feature` — Vision archive checks

## Collector

```bash
cargo run -p parangsae-collector -- vision \
  --dataset um-kline --from 2026-09-01 --to 2026-09-02 --data-dir ./data
cargo run -p parangsae-collector -- verify --data-dir ./data
```

`--dataset all` runs the eight Tier-1 datasets. `--dataset` also accepts the Vision folder names (`klines`, `markPriceKlines`, `fundingRate`, `metrics`, `bookDepth`). The default data directory is `./data`, which is gitignored. `verify` reads local files only.

A checksum mismatch, a row-level fault, a conflict, or an unexpected gap makes the process exit 1. The same key with the same payload is a no-op. The same key with a different payload is recorded and the stored row is left in place.

- `AGENTS.md` — rules for agents working in this repo

## Checks

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

GitHub Actions also runs TLC and Kani. A green run is those checks, not a performance or live-readiness claim.
