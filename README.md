# parangsae

parangsae is a from-scratch Rust research stack for Binance USD-M BTCUSDT. Holds are mid-frequency: tens of minutes to days, on bars from 1 minute to 4 hours.

Formal specs are TLA+, checked by TLC. Behavior specs are Gherkin, checked by cucumber-rs. Live trading is not implemented. `crates/execution` has no order client.

The previous Python implementation is at the `python-legacy` tag (commit `8713f024afa18bcb9fd8e7cae573acffab6bdb7e`). Do not run it and do not port it.

## Layout

- `crates/core` — `Price`, `Qty`, `Symbol`, the three timestamps, the `Causal` gate, kline time checks
- `crates/collector` — module boundaries only (no client)
- `crates/signals`, `crates/regime`, `crates/execution` — skeletons
- `specs/tla/IngestSegment.tla` — ingest gap / duplicate model
- `specs/features/kline_integrity.feature` — kline clock and grid
- `AGENTS.md` — rules for agents working in this repo

## Checks

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

GitHub Actions also runs TLC and Kani. A green run is those checks, not a performance or live-readiness claim.
