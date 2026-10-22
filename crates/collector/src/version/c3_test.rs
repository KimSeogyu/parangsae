//! C3: version store, v2 conflict lines, and the resolution registry.

use std::path::Path;

use super::resolve::{self, ResolutionError};
use super::source::Source;
use super::versions::{logged_conflicts, record_kline_archive};
use crate::civil::CivilDay;
use crate::normalize::UM_KLINE_CLOCK;
use crate::vision::{parse_klines, Cadence, TickDomain};
use parangsae_core::{Receipt, RunId, Symbol};

const MONTHLY_SHA: &str = "63aa156170b2316927ae4c4928cc78866934a02b4bbc14c730b5fe4790383a80";
const DAILY_10_SHA: &str = "60fbc08c0a73afa5f04d8bc9b8b960cb782435147c1a1080a2b449714db1ea20";
const DAILY_14_SHA: &str = "8c2dbbf36dfa6ca479e7125c90ec26488b6d402446bdd823df819d80bb325be5";
const MONTHLY_PATH: &str = "data/futures/um/monthly/klines/BTCUSDT/1m/BTCUSDT-1m-2023-11.zip";
const DAILY_10_PATH: &str = "data/futures/um/daily/klines/BTCUSDT/1m/BTCUSDT-1m-2023-11-10.zip";
const DAILY_14_PATH: &str = "data/futures/um/daily/klines/BTCUSDT/1m/BTCUSDT-1m-2023-11-14.zip";

fn show(err: impl std::fmt::Display) -> String {
    err.to_string()
}

fn sha(text: &str) -> [u8; 32] {
    let bytes = hex::decode(text).expect("sha");
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    out
}

fn source(path: &str, digest: &str, cadence: Cadence, modified: &str) -> Source {
    Source {
        path: path.to_owned(),
        sha256: sha(digest),
        cadence,
        schema_version: 2,
        last_modified: Some(modified.to_owned()),
        fetched_at_run: RunId::from_bytes([7; 16]),
    }
}

fn rows(name: &str) -> Vec<crate::vision::KlineFact> {
    let csv = std::fs::read_to_string(format!(
        "{}/../../fixtures/vision/integrity/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|err| panic!("{name}: {err}"));
    parse_klines(
        &csv,
        &Symbol::new("BTCUSDT").expect("symbol"),
        UM_KLINE_CLOCK,
        TickDomain::Price,
        Receipt::Archive {
            fetched_at_run: RunId::from_bytes([7; 16]),
        },
        true,
    )
    .expect("parse")
    .rows
}

fn ingest_pair(dir: &Path) -> Result<(), String> {
    let monthly = source(
        MONTHLY_PATH,
        MONTHLY_SHA,
        Cadence::Monthly,
        "Mon, 08 Jan 2024 08:33:32 GMT",
    );
    let daily10 = source(
        DAILY_10_PATH,
        DAILY_10_SHA,
        Cadence::Daily,
        "Mon, 13 Nov 2023 07:51:38 GMT",
    );
    let daily14 = source(
        DAILY_14_PATH,
        DAILY_14_SHA,
        Cadence::Daily,
        "Wed, 15 Nov 2023 06:56:32 GMT",
    );
    record_kline_archive(
        dir,
        "um-kline",
        rows("um_monthly_2023-11-10_slice.csv"),
        &monthly,
    )
    .map_err(show)?;
    record_kline_archive(
        dir,
        "um-kline",
        rows("um_daily_2023-11-10_slice.csv"),
        &daily10,
    )
    .map_err(show)?;
    record_kline_archive(
        dir,
        "um-kline",
        rows("um_monthly_2023-11-14_slice.csv"),
        &monthly,
    )
    .map_err(show)?;
    record_kline_archive(
        dir,
        "um-kline",
        rows("um_daily_2023-11-14_slice.csv"),
        &daily14,
    )
    .map_err(show)?;
    Ok(())
}

fn registry_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resolutions")
}

#[test]
fn reviewed_registry_matches_the_version_store_and_unreviewed_entries_are_refused(
) -> Result<(), String> {
    let registry = resolve::load(&registry_root()).map_err(show)?;
    if registry.entries().len() != 2 {
        return Err(format!("entries {}", registry.entries().len()));
    }
    let dir = tempfile::tempdir().map_err(show)?;
    ingest_pair(dir.path())?;
    registry.check_complete(dir.path()).map_err(show)?;

    let mut broken = registry.entries()[0].keys_digest.clone();
    broken.replace_range(0..1, "0");
    let mut text = std::fs::read_to_string(registry_root().join("vision/um-kline.BTCUSDT.jsonl"))
        .map_err(show)?;
    let original = registry.entries()[0].keys_digest.clone();
    text = text.replacen(&original, &broken, 1);
    let bad = dir.path().join("resolutions");
    std::fs::create_dir_all(bad.join("vision")).map_err(show)?;
    std::fs::write(bad.join("vision/um-kline.BTCUSDT.jsonl"), &text).map_err(show)?;
    let loaded = resolve::load(&bad).map_err(show)?;
    match loaded.check_complete(dir.path()) {
        Err(ResolutionError {
            check: "resolution_mismatch",
            ..
        }) => {}
        other => return Err(format!("digest tamper: {other:?}")),
    }
    text =
        text.replacen(&broken, &original, 1)
            .replacen("\"key_count\":100", "\"key_count\":99", 1);
    std::fs::write(bad.join("vision/um-kline.BTCUSDT.jsonl"), text).map_err(show)?;
    let loaded = resolve::load(&bad).map_err(show)?;
    match loaded.check_complete(dir.path()) {
        Err(ResolutionError {
            check: "resolution_mismatch",
            ..
        }) => {}
        other => return Err(format!("count tamper: {other:?}")),
    }

    let placeholder = concat!(
        r#"{"id":"x","dataset":"um-kline","symbol":"BTCUSDT","interval_us":"60000000","#,
        r#""pk_first_us":"1","pk_last_us":"1","key_count":1,"version_set":[{"path":"p","sha256":""#,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        r#"","cadence":"daily"}],"choose_sha256":""#,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        r#"","keys_digest":"ab","evidence":{},"reviewed_by":"<someone>","reviewed_at":"2026-10-22T13:25:00Z"}"#
    );
    std::fs::write(
        bad.join("vision/placeholder.jsonl"),
        format!("{placeholder}\n"),
    )
    .map_err(show)?;
    match resolve::load(&bad) {
        Err(ResolutionError {
            check: "resolution_unreviewed",
            ..
        }) => {}
        other => return Err(format!("placeholder: {other:?}")),
    }
    let missing = placeholder.replace(r#""reviewed_by":"<someone>","#, "");
    std::fs::write(bad.join("vision/placeholder.jsonl"), format!("{missing}\n")).map_err(show)?;
    match resolve::load(&bad) {
        Err(ResolutionError {
            check: "resolution_unreviewed",
            ..
        }) => Ok(()),
        other => Err(format!("missing reviewed_by: {other:?}")),
    }
}

#[test]
fn disputed_keys_record_both_sources_once() -> Result<(), String> {
    let dir = tempfile::tempdir().map_err(show)?;
    ingest_pair(dir.path())?;
    let log = std::fs::read_to_string(dir.path().join("conflicts/um-kline.jsonl")).map_err(show)?;
    let lines: Vec<&str> = log.lines().filter(|line| !line.is_empty()).collect();
    if lines.len() != 105 {
        return Err(format!("lines {}", lines.len()));
    }
    let reference = lines
        .iter()
        .find(|line| line.contains("\"key\":\"1699628820000000\""))
        .ok_or("reference key missing")?;
    let value: serde_json::Value = serde_json::from_str(reference).map_err(show)?;
    if value["v"] != 2 || value["class"] != "value_by_log" {
        return Err(reference.to_string());
    }
    let set = value["version_set_sha256"]
        .as_array()
        .ok_or("version set")?;
    let shas: Vec<&str> = set.iter().filter_map(|item| item.as_str()).collect();
    if shas != [DAILY_10_SHA, MONTHLY_SHA] {
        return Err(format!("unsorted or wrong set {shas:?}"));
    }
    let stored = value["stored_sources"].to_string();
    let rejected = value["rejected_sources"].to_string();
    if !stored.contains(MONTHLY_PATH) || !rejected.contains(DAILY_10_PATH) {
        return Err(format!("{stored} {rejected}"));
    }
    ingest_pair(dir.path())?;
    let again =
        std::fs::read_to_string(dir.path().join("conflicts/um-kline.jsonl")).map_err(show)?;
    if again.lines().filter(|line| !line.is_empty()).count() != 105 {
        return Err("re-ingest appended a line".into());
    }
    let day = CivilDay::new(2023, 11, 10).ok_or("day")?;
    let listed = logged_conflicts(dir.path(), "um-kline", "BTCUSDT", day).map_err(show)?;
    if listed.len() != 100 || listed.iter().any(|line| line.class != "value_by_log") {
        return Err(format!("listed {}", listed.len()));
    }
    let legacy = "{\"dataset\":\"um-kline\",\"symbol\":\"BTCUSDT\",\"key\":\"1699628820000000\",\"stored\":\"open=1 schema=1\",\"rejected\":\"open=1 schema=2\"}\n";
    std::fs::write(
        dir.path().join("conflicts/um-kline.jsonl"),
        format!("{legacy}{log}"),
    )
    .map_err(show)?;
    let listed = logged_conflicts(dir.path(), "um-kline", "BTCUSDT", day).map_err(show)?;
    if listed.len() != 100 {
        return Err(format!("legacy duplicated the key: {}", listed.len()));
    }
    let stale = reference.replace(DAILY_10_SHA, &"ab".repeat(32));
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(dir.path().join("conflicts/um-kline.jsonl"))
        .map_err(show)?;
    writeln!(file, "{stale}").map_err(show)?;
    let listed = logged_conflicts(dir.path(), "um-kline", "BTCUSDT", day).map_err(show)?;
    if listed.len() != 100 {
        return Err(format!("stale version set was listed: {}", listed.len()));
    }
    let other = [MONTHLY_SHA.to_owned()];
    let registry = resolve::load(&registry_root()).map_err(show)?;
    if registry
        .lookup("um-kline", "BTCUSDT", 1_699_628_820_000_000, &other)
        .is_some()
    {
        return Err("a different archive set applied".into());
    }
    let active = vec![DAILY_10_SHA.to_owned(), MONTHLY_SHA.to_owned()];
    if registry
        .lookup("um-kline", "BTCUSDT", 1_699_628_820_000_000, &active)
        .is_none()
    {
        return Err("the reviewed entry did not apply to its own archive set".into());
    }
    Ok(())
}
