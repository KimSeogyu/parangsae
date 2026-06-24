# Migration Manifest

This repository is a new integration repo. It preserves the three source repos by reference and selective migration; it does not overwrite them.

## Source repositories

| Source | Role in unified app | Preservation decision |
|---|---|---|
| `/Users/***REMOVED***/Development/parangsae` | Research/news/reporting/Telegram-style notification ideas | Reuse concepts and selected scripts later; do not copy `.env`, `.secrets`, generated previews, or n8n data. |
| `/Users/***REMOVED***/Development/parangsae-engine` | Deterministic simulation/event log/execution/ledger/UI concepts | Reuse event-log/ledger invariants as core contract/replay spine; do not copy `target`, `ui/node_modules`, or generated `ui/dist`. |
| `/Users/***REMOVED***/Development/parangsae-quant` | Binance research/backtest/portfolio/risk foundation | Reuse research ideas/config and strategy shape; do not copy `.venv`, caches, raw data, or secrets. |

## Live source status at creation

Generated with `scripts/write_migration_manifest.py` during setup. The source repos had dirty/untracked work, so this repo was created independently.

```text
parangsae: dirty tracked files observed in preflight (.gitignore, launchd/com.sto-team.daily-update.plist)
parangsae-engine: dirty tracked and untracked implementation/build/UI artifacts observed in preflight
parangsae-quant: ahead 1 with dirty tracked and untracked research/Binance Vision files observed in preflight
```

## Explicit exclusions

- `.git/`
- `.env`, `.secrets/`, credentials, API keys
- `.venv/`, `node_modules/`, `target/`, `dist/`
- raw market data, parquet caches, generated PDFs/images/previews
- Python/Rust/JS caches

## First-milestone migrated concepts

- Product-family split replaces the old single crypto-native universe policy.
- Shared core contracts are the source of truth for signals, portfolios, orders, risk decisions, execution events, and event envelopes.
- Balanced risk defaults are encoded in `configs/risk.balanced.yaml`.
- Binance live order paths are default-disabled and guarded.
