# parangsae

A Rust research and execution prototype for systematic trading, starting with **Binance USD-M BTCUSDT**. The target horizon is mid-frequency: holds of tens of minutes to days, using bars from 1 minute to 4 hours.

The project explores **specification-guided development with AI agents**. The goal is correct-by-construction boundaries: use formal models, Rust types, and ownership to prevent callers from constructing invalid states or bypassing permitted transitions.

**Research only.** Live trading is not implemented. The execution crate has no network order client, and its paper venue stub rejects every order. A working paper adapter, profitable strategy, and live-readiness are not claimed.

## Design direction

Prefer removing an invalid operation from an API over adding another checker for it.

- **Model state and transitions explicitly.** TLA+ models describe selected behaviors; private state and narrow transition APIs constrain callers.
- **Make data boundaries carry meaning.** Fixed-point price and quantity types, distinct timestamps, and `Causal<T>` views keep domain assumptions visible. The causal gate admits a value only when its declared availability is no later than the decision time.
- **Keep effects and ownership narrow.** Accounting state belongs to its ledger; external data is parsed at the boundary. Dynamic conditions still require runtime validation.
- **Use verification to test those boundaries.** TLC, Kani, Gherkin, and property tests cover particular models, harnesses, and examples. They do not prove the entire system correct or establish trading edge.

Spec-derived transition APIs are a **next step**, tracked in [#25](https://github.com/KimSeogyu/parangsae/issues/25). The current lifecycle transitions are hand-written; the project does not yet generate their implementation from TLA+.

## What is here

| Component | Current scope |
| --- | --- |
| [`crates/core`](crates/core) | Fixed-point types, timestamps, causal access, bar resampling, and instrument rules |
| [`crates/collector`](crates/collector) | Public Binance Vision archive ingestion, validation, and Parquet storage; REST/WS modules are boundaries, not working clients |
| [`crates/signals`](crates/signals) · [`crates/strategies`](crates/strategies) | Causal signals, features, and initial strategy families |
| [`crates/sim`](crates/sim) · [`crates/backtest`](crates/backtest) | Fixed-point accounting, event-driven backtesting, explicit fees/funding/slippage, and walk-forward tooling |
| [`crates/execution`](crates/execution) | Offline order lifecycle and a sealed, reject-only paper venue stub |
| [`crates/regime`](crates/regime) | Permission-layer scaffold; no classifier implemented |
| [`specs`](specs) · [`spec/arch`](spec/arch) · [`xtask`](xtask) | Behavioral models and scenarios, architecture policy, and supporting checks |

Point-in-time correctness also depends on the source data's timestamp semantics. See the [research read-path contract](docs/data/research-read-path.md) for availability rules, coverage, exclusions, and holdout boundaries.

## Collector

```bash
cargo run -p parangsae-collector -- vision \
  --dataset um-kline --from 2024-09-01 --to 2024-09-02 --data-dir ./data
cargo run -p parangsae-collector -- verify --data-dir ./data
```

The collector downloads public `data.binance.vision` objects. No API key or order route is used.

`--dataset all` runs the eight Tier-1 datasets. `--dataset` also accepts the Vision folder names (`klines`, `markPriceKlines`, `fundingRate`, `metrics`, `bookDepth`). The default data directory is `./data`, which is gitignored. `verify` reads local files only.

Detected checksum mismatches, row-level faults, conflicts, and gaps make the process exit 1. A successful verification is limited to the selected local artifacts; it does not establish source completeness or point-in-time correctness. See [#9](https://github.com/KimSeogyu/parangsae/issues/9) for the missing-partition regression and its resolution. The same key with the same payload is a no-op. The same key with a different payload is recorded and the stored row is left in place.

## Reading guide

- [Agent rules and full check commands](AGENTS.md)
- [Research protocols and amendments](research/README.md)
- [Order lifecycle model and its verification limits](specs/tla/execution/README.md)
- [Ingest model](specs/tla/IngestSegment.tla) · [Fact-write model](specs/tla/ManifestWrite.tla)
- [Kline scenarios](specs/features/kline_integrity.feature) · [Vision scenarios](specs/features/collector/vision_integrity.feature)
- [Research knowledge hub proposal — draft PR #13, not merged](https://github.com/KimSeogyu/parangsae/pull/13)

The previous Python implementation is preserved at the [`python-legacy` tag](https://github.com/KimSeogyu/parangsae/tree/python-legacy) (commit `8713f024afa18bcb9fd8e7cae573acffab6bdb7e`). Do not run it or port it.

## Checks

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

[CI](.github/workflows/ci.yml) also checks architecture policy and runs path-gated TLC and Kani jobs. Follow [AGENTS.md](AGENTS.md) for the full validation workflow. A green run means the checks that ran passed; path-skipped proof jobs were not executed. It is not a performance or live-readiness verdict.
