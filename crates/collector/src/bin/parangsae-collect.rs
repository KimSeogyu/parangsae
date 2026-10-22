//! Binance Vision backfill. Public archives only. No API key and no order route.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use parangsae_collector::civil::{parse_day, DayRange};
use parangsae_collector::fetch::HttpGet;
use parangsae_collector::run::{self, Provenance, RunRequest, VerifyFilter};
use parangsae_collector::version::{
    migrate_receipts, parse_version_id, publish, DatasetId, LoadError, ManifestDigest,
    PublishError, PublishRequest, VerifyMode, VersionIdError, VersionStore,
};
use parangsae_collector::vision::VisionDataset;
use parangsae_core::Symbol;

#[derive(Parser)]
#[command(name = "parangsae-collect")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Download and ingest Binance Vision archives for a closed date range.
    Vision(VisionArgs),
    /// Re-check local checksums, parquet rows, and invariants. Does not use the network.
    Verify(VerifyArgs),
    /// Map rows/v1 `received_ts_us` values onto archive runs. Default is a dry run.
    MigrateReceipts(MigrateArgs),
    /// Publish one dataset version from the local store. Does not use the network.
    Publish(PublishArgs),
    /// Load one published version and run its validators. Does not use the network.
    VerifyVersion(VerifyVersionArgs),
}

#[derive(Parser)]
struct VisionArgs {
    /// Dataset name, a Vision folder alias, or `all`.
    #[arg(long)]
    dataset: String,
    /// First UTC day, inclusive. YYYY-MM-DD.
    #[arg(long)]
    from: String,
    /// Last UTC day, inclusive. YYYY-MM-DD.
    #[arg(long)]
    to: String,
    #[arg(long, default_value = "./data")]
    data_dir: PathBuf,
    #[arg(long, default_value = "BTCUSDT")]
    symbol: String,
    #[arg(long, default_value_t = 2)]
    jobs: usize,
    #[arg(long, default_value_t = 200)]
    min_interval_ms: u64,
}

#[derive(Parser)]
struct VerifyArgs {
    #[arg(long, default_value = "./data")]
    data_dir: PathBuf,
    /// Dataset name, repeatable. Same names as `vision --dataset`.
    #[arg(long = "dataset")]
    dataset: Vec<String>,
    /// First UTC day, inclusive. YYYY-MM-DD.
    #[arg(long)]
    from: Option<String>,
    /// Last UTC day, inclusive. YYYY-MM-DD.
    #[arg(long)]
    to: Option<String>,
}

#[derive(Parser)]
struct MigrateArgs {
    #[arg(long, default_value = "./data")]
    data_dir: PathBuf,
    /// Rewrite v1 partitions. Omit for a dry run, which writes nothing.
    #[arg(long, default_value_t = false)]
    apply: bool,
}

#[derive(Parser)]
struct PublishArgs {
    /// Dataset id, `source.market.dataset.symbol`.
    #[arg(long)]
    dataset: String,
    /// First UTC day, inclusive. YYYY-MM-DD.
    #[arg(long)]
    from: String,
    /// Last UTC day, inclusive. YYYY-MM-DD.
    #[arg(long)]
    to: String,
    #[arg(long, default_value = "./data")]
    data_dir: PathBuf,
}

#[derive(Parser)]
struct VerifyVersionArgs {
    /// `<dataset id>@<64 lowercase hex root>`. Aliases are refused.
    #[arg(long)]
    version: String,
    /// Manifest digest, 64 lowercase hex characters.
    #[arg(long)]
    manifest_digest: String,
    /// First UTC day, inclusive. YYYY-MM-DD.
    #[arg(long)]
    from: String,
    /// Last UTC day, inclusive. YYYY-MM-DD.
    #[arg(long)]
    to: String,
    #[arg(long, default_value = "./data")]
    data_dir: PathBuf,
    /// Ignore the verified cache and re-run every validator.
    #[arg(long, default_value_t = false)]
    full: bool,
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Vision(args) => vision(args),
        Command::Verify(args) => verify(args),
        Command::MigrateReceipts(args) => migrate(args),
        Command::Publish(args) => publish_version(args),
        Command::VerifyVersion(args) => verify_version(args),
    }
}

fn vision(args: VisionArgs) -> ExitCode {
    let datasets = match datasets(&args.dataset) {
        Ok(datasets) => datasets,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let symbol = match Symbol::new(&args.symbol) {
        Ok(symbol) => symbol,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let from = match parse_day(&args.from) {
        Ok(day) => day,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let to = match parse_day(&args.to) {
        Ok(day) => day,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let range = match DayRange::new(from, to) {
        Ok(range) => range,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let run_id = match run::generate_run_id() {
        Ok(run_id) => run_id,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let started_at = match run::wall_now() {
        Ok(ts) => ts,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let client = match HttpGet::new() {
        Ok(client) => client,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let request = RunRequest {
        datasets,
        symbol,
        range,
        data_dir: args.data_dir,
        jobs: args.jobs,
        min_interval: Duration::from_millis(args.min_interval_ms),
        run: run_id,
        started_at,
    };
    match run::run_vision(&request, &client) {
        Ok(rows) => print_rows(&rows),
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(2)
        }
    }
}

fn verify(args: VerifyArgs) -> ExitCode {
    let filter = match VerifyFilter::parse(&args.dataset, args.from.as_deref(), args.to.as_deref())
    {
        Ok(filter) => filter,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    match run::verify(&args.data_dir, &filter) {
        Ok(report) => {
            println!("verify {report}");
            if report.ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(2)
        }
    }
}

fn publish_version(args: PublishArgs) -> ExitCode {
    let dataset = match DatasetId::parse(&args.dataset) {
        Ok(dataset) => dataset,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let range = match day_range(&args.from, &args.to) {
        Ok(range) => range,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let run = match run::generate_run_id() {
        Ok(run) => run,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let built_at = match run::wall_now() {
        Ok(built_at) => built_at,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let provenance = Provenance::from_build();
    match publish(&PublishRequest {
        data_dir: &args.data_dir,
        dataset: &dataset,
        range,
        provenance: &provenance,
        run,
        built_at,
    }) {
        Ok(published) => {
            println!(
                "publish root={} manifest={} attestation={}",
                published.root.to_hex(),
                published.manifest_digest.to_hex(),
                hex_of(published.attestation_digest)
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            publish_status(&err)
        }
    }
}

fn verify_version(args: VerifyVersionArgs) -> ExitCode {
    let (dataset, root) = match parse_version_id(&args.version) {
        Ok(parsed) => parsed,
        Err(VersionIdError::Alias) => {
            eprintln!("version id alias is refused");
            return ExitCode::from(2);
        }
        Err(VersionIdError::Malformed) => {
            eprintln!("version id is malformed");
            return ExitCode::from(2);
        }
    };
    let digest = match ManifestDigest::parse_hex(&args.manifest_digest) {
        Ok(digest) => digest,
        Err(_) => {
            eprintln!("manifest digest is malformed");
            return ExitCode::from(2);
        }
    };
    let range = match day_range(&args.from, &args.to) {
        Ok(range) => range,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let store = match VersionStore::open(&args.data_dir) {
        Ok(store) => store,
        Err(err) => {
            eprintln!("{err}");
            return load_status(&err);
        }
    };
    let mode = if args.full {
        VerifyMode::Full
    } else {
        VerifyMode::Cached
    };
    match store.load_with(&dataset, root, digest, range, mode) {
        Ok(verified) => {
            println!(
                "verify-version rows_checked leaves={} cache_hits={}",
                verified.leaves_verified(),
                verified.cache_hits()
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            load_status(&err)
        }
    }
}

fn day_range(from: &str, to: &str) -> Result<DayRange, parangsae_collector::Error> {
    let from = parse_day(from)?;
    let to = parse_day(to)?;
    DayRange::new(from, to)
}

fn hex_of(bytes: [u8; 32]) -> String {
    const HEX: [char; 16] = [
        '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
    ];
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(HEX[usize::from(byte >> 4)]);
        out.push(HEX[usize::from(byte & 0x0f)]);
    }
    out
}

/// Every [`LoadError`] is a failed verification. A new variant fails this match.
fn load_status(err: &LoadError) -> ExitCode {
    match err {
        LoadError::UnknownVersion
        | LoadError::ManifestDigestMismatch
        | LoadError::ManifestRootMismatch
        | LoadError::ManifestOrder
        | LoadError::PartitionMissing { .. }
        | LoadError::PartitionHashMismatch { .. }
        | LoadError::SchemaMismatch { .. }
        | LoadError::PkOrder { .. }
        | LoadError::AvailableTsRule { .. }
        | LoadError::RowInvariant { .. }
        | LoadError::PartitionStatsMismatch { .. }
        | LoadError::RangeOutsideVersion
        | LoadError::HoldoutRefused
        | LoadError::UnlistedConflict { .. }
        | LoadError::LegacyObjectLine { .. }
        | LoadError::SpecVersionMismatch { .. }
        | LoadError::SupersededVersion { .. }
        | LoadError::StalePayloadVersion { .. }
        | LoadError::UnknownPayloadVersion { .. }
        | LoadError::ManifestStructure { .. }
        | LoadError::UnknownRuleVersion { .. }
        | LoadError::ZeroOiStored { .. }
        | LoadError::NegativeOiStored { .. }
        | LoadError::NonPositiveRatioStored { .. }
        | LoadError::MetricMissingMismatch { .. }
        | LoadError::Io(_) => ExitCode::from(1),
    }
}

/// Every [`PublishError`] refuses the publish. A new variant fails this match.
fn publish_status(err: &PublishError) -> ExitCode {
    match err {
        PublishError::UnknownProvenance
        | PublishError::DirtyProvenance
        | PublishError::StaleGapCache { .. }
        | PublishError::SnapshotCorrupt
        | PublishError::ManifestExists
        | PublishError::UnsupportedDataset
        | PublishError::SpecVersionMismatch
        | PublishError::Failed(_) => ExitCode::from(1),
    }
}

fn migrate(args: MigrateArgs) -> ExitCode {
    let provenance = Provenance::from_build();
    match migrate_receipts(&args.data_dir, args.apply, &provenance) {
        Ok(report) => {
            let mode = if args.apply { "apply" } else { "dry-run" };
            print!("migrate-receipts {mode} {report}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            if err.check_name().is_some() {
                ExitCode::from(1)
            } else {
                ExitCode::from(2)
            }
        }
    }
}

fn print_rows(rows: &[(VisionDataset, run::RunSummary)]) -> ExitCode {
    let mut failed = false;
    for (dataset, summary) in rows {
        println!("{dataset} {summary}");
        if !summary.ok() {
            failed = true;
        }
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn datasets(text: &str) -> Result<Vec<VisionDataset>, String> {
    if text == "all" {
        return Ok(VisionDataset::ALL.to_vec());
    }
    VisionDataset::parse(text)
        .map(|dataset| vec![dataset])
        .ok_or_else(|| format!("unknown dataset {text}"))
}

#[cfg(test)]
mod tests {
    use std::process::ExitCode;

    use parangsae_collector::civil::CivilDay;
    use parangsae_collector::version::{LoadError, PublishError};

    use super::{load_status, publish_status};

    fn day() -> Result<CivilDay, String> {
        CivilDay::new(2021, 6, 1).ok_or_else(|| "2021-06-01".to_owned())
    }

    #[test]
    fn every_load_error_exits_1() -> Result<(), String> {
        let date = day()?;
        let errors = [
            LoadError::UnknownVersion,
            LoadError::ManifestDigestMismatch,
            LoadError::ManifestRootMismatch,
            LoadError::ManifestOrder,
            LoadError::PartitionMissing { date },
            LoadError::PartitionHashMismatch { date },
            LoadError::SchemaMismatch { date },
            LoadError::PkOrder { date },
            LoadError::AvailableTsRule { date },
            LoadError::RowInvariant {
                date,
                check: "symbol",
            },
            LoadError::PartitionStatsMismatch { date },
            LoadError::RangeOutsideVersion,
            LoadError::HoldoutRefused,
            LoadError::UnlistedConflict {
                key: "1".to_owned(),
            },
            LoadError::LegacyObjectLine {
                url: "https://example.invalid/a".to_owned(),
            },
            LoadError::SpecVersionMismatch {
                dataset: "um-kline".to_owned(),
                found: "parangsae/dataset-manifest/v2.2".to_owned(),
            },
            LoadError::SupersededVersion {
                dataset_id: "binance-vision.um.metrics.BTCUSDT".to_owned(),
                root: parangsae_collector::version::VersionRoot::from_bytes([0; 32]),
                successor_root: None,
                rule: parangsae_collector::version::RuleVersion::MetricsSpecV2_1,
            },
            LoadError::StalePayloadVersion {
                date,
                found: Some("payload/v2".to_owned()),
                required: "payload/v3".to_owned(),
            },
            LoadError::UnknownPayloadVersion {
                found: Some("payload/v4".to_owned()),
            },
            LoadError::UnknownPayloadVersion { found: None },
            LoadError::ManifestStructure {
                pointer: "/schema".to_owned(),
                expected: "object",
            },
            LoadError::UnknownRuleVersion {
                field: "metric_missing_rules",
                found: None,
            },
            LoadError::ZeroOiStored {
                date,
                pk: 1,
                column: "sum_open_interest",
            },
            LoadError::NegativeOiStored {
                date,
                pk: 1,
                column: "sum_open_interest",
            },
            LoadError::NonPositiveRatioStored {
                date,
                pk: 1,
                column: "count_long_short_ratio",
            },
            LoadError::MetricMissingMismatch { date },
            LoadError::Io("io".to_owned()),
        ];
        for err in &errors {
            if load_status(err) != ExitCode::from(1) {
                return Err(format!("{err} did not exit 1"));
            }
        }
        Ok(())
    }

    #[test]
    fn every_publish_error_exits_1() -> Result<(), String> {
        let date = day()?;
        let errors = [
            PublishError::UnknownProvenance,
            PublishError::DirtyProvenance,
            PublishError::StaleGapCache { day: date },
            PublishError::SnapshotCorrupt,
            PublishError::ManifestExists,
            PublishError::UnsupportedDataset,
            PublishError::SpecVersionMismatch,
            PublishError::Failed("detail".to_owned()),
        ];
        for err in &errors {
            if publish_status(err) != ExitCode::from(1) {
                return Err(format!("{err} did not exit 1"));
            }
        }
        Ok(())
    }
}
