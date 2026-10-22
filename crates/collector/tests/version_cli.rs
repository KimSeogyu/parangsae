//! CLI exit codes for `publish` and `verify-version`. Temp directories only.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use parangsae_collector::civil::{day_of, CivilDay, DayRange};
use parangsae_collector::run::Provenance;
use parangsae_collector::store::{self, load_klines};
use parangsae_collector::version::{publish, DatasetId, PublishRequest};
use parangsae_collector::vision::{parse_klines, TickDomain};
use parangsae_core::{Qty, Receipt, RunId, Symbol, UtcMicros};

fn show(err: impl std::fmt::Display) -> String {
    err.to_string()
}

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_parangsae-collect"))
}

fn run(args: &[&str]) -> Result<std::process::Output, String> {
    bin().args(args).output().map_err(show)
}

fn code(output: &std::process::Output) -> Result<i32, String> {
    output
        .status
        .code()
        .ok_or_else(|| "process exited without a status".to_owned())
}

#[test]
fn usage_errors_exit_2_and_a_missing_version_exits_1() -> Result<(), String> {
    let missing_args = run(&["publish"])?;
    assert_eq!(
        code(&missing_args)?,
        2,
        "{}",
        String::from_utf8_lossy(&missing_args.stderr)
    );

    let dir = tempfile::tempdir().map_err(show)?;
    let root = dir.path().to_str().ok_or("path is not utf-8")?;
    let digest = "ab".repeat(32);
    let alias = run(&[
        "verify-version",
        "--data-dir",
        root,
        "--version",
        "binance-vision.um.um-kline.BTCUSDT@latest",
        "--manifest-digest",
        &digest,
        "--from",
        "2021-06-01",
        "--to",
        "2021-06-01",
    ])?;
    assert_eq!(
        code(&alias)?,
        2,
        "{}",
        String::from_utf8_lossy(&alias.stderr)
    );

    let unknown = run(&[
        "verify-version",
        "--data-dir",
        root,
        "--version",
        "binance-vision.um.um-kline.BTCUSDT@0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "--manifest-digest",
        &digest,
        "--from",
        "2021-06-01",
        "--to",
        "2021-06-01",
    ])?;
    assert_eq!(
        code(&unknown)?,
        1,
        "{}",
        String::from_utf8_lossy(&unknown.stderr)
    );
    let stderr = String::from_utf8_lossy(&unknown.stderr);
    assert!(stderr.contains("UnknownVersion"), "{stderr}");
    Ok(())
}

#[test]
fn verify_version_refuses_holdout_and_a_tampered_snapshot() -> Result<(), String> {
    let published = publish_fixture()?;
    let root = published
        .dir
        .to_str()
        .ok_or("path is not utf-8")?
        .to_owned();
    let held = run(&[
        "verify-version",
        "--data-dir",
        &root,
        "--version",
        &published.version,
        "--manifest-digest",
        &published.digest,
        "--from",
        "2025-10-01",
        "--to",
        "2025-10-01",
    ])?;
    assert_eq!(code(&held)?, 1, "{}", String::from_utf8_lossy(&held.stderr));
    assert!(
        String::from_utf8_lossy(&held.stderr).contains("HoldoutRefused"),
        "{}",
        String::from_utf8_lossy(&held.stderr)
    );

    let ok = run(&[
        "verify-version",
        "--data-dir",
        &root,
        "--version",
        &published.version,
        "--manifest-digest",
        &published.digest,
        "--from",
        "2021-06-01",
        "--to",
        "2021-06-01",
    ])?;
    assert_eq!(code(&ok)?, 0, "{}", String::from_utf8_lossy(&ok.stderr));

    bump_snapshot(&published.dir)?;
    let tampered = run(&[
        "verify-version",
        "--data-dir",
        &root,
        "--version",
        &published.version,
        "--manifest-digest",
        &published.digest,
        "--from",
        "2021-06-01",
        "--to",
        "2021-06-01",
        "--full",
    ])?;
    assert_eq!(
        code(&tampered)?,
        1,
        "{}",
        String::from_utf8_lossy(&tampered.stderr)
    );
    assert!(
        String::from_utf8_lossy(&tampered.stderr).contains("PartitionHashMismatch"),
        "{}",
        String::from_utf8_lossy(&tampered.stderr)
    );
    Ok(())
}

struct Published {
    dir: PathBuf,
    version: String,
    digest: String,
}

fn publish_fixture() -> Result<Published, String> {
    let csv = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/vision/um_kline_2021-06-01_headerless.csv"),
    )
    .map_err(show)?;
    let parsed = parse_klines(
        &csv,
        &Symbol::new("BTCUSDT").map_err(show)?,
        parangsae_collector::normalize::UM_KLINE_CLOCK,
        TickDomain::Price,
        Receipt::Archive {
            fetched_at_run: RunId::from_bytes([1; 16]),
        },
        true,
    )
    .map_err(show)?;
    let day = day_of(parsed.rows[0].open_time).map_err(show)?;
    let dir = tempfile::tempdir().map_err(show)?.keep();
    store::upsert_klines(&dir, "um-kline", parsed.rows).map_err(show)?;
    let dataset = DatasetId::parse("binance-vision.um.um-kline.BTCUSDT").map_err(show)?;
    let provenance = Provenance {
        git_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        dirty: false,
    };
    let published = publish(&PublishRequest {
        data_dir: &dir,
        dataset: &dataset,
        range: DayRange::new(day, day).map_err(show)?,
        provenance: &provenance,
        run: RunId::from_bytes([4; 16]),
        built_at: UtcMicros::from_micros(1),
    })
    .map_err(show)?;
    Ok(Published {
        dir,
        version: format!(
            "binance-vision.um.um-kline.BTCUSDT@{}",
            published.root.to_hex()
        ),
        digest: published.manifest_digest.to_hex(),
    })
}

fn bump_snapshot(dir: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir.join("snapshots")).map_err(show)? {
        let path = entry.map_err(show)?.path().join("part.parquet");
        if path.is_file() {
            files.push(path);
        }
    }
    if files.len() != 1 {
        return Err(format!("expected one snapshot, found {}", files.len()));
    }
    let path = &files[0];
    let mut permissions = fs::metadata(path).map_err(show)?.permissions();
    permissions.set_mode(0o644);
    fs::set_permissions(path, permissions).map_err(show)?;
    let day = CivilDay::new(2021, 6, 1).ok_or_else(|| "2021-06-01".to_owned())?;
    let mut rows = load_klines(dir, "um-kline", "BTCUSDT", day).map_err(show)?;
    let bumped = rows
        .first_mut()
        .ok_or_else(|| "snapshot has no row".to_owned())?;
    bumped.volume = Qty::from_ticks(bumped.volume.ticks().saturating_add(1)).map_err(show)?;
    store::write_klines(path, "um-kline", &rows).map_err(show)?;
    Ok(())
}
