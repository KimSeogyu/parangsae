//! Version store: one row per `(pk, source sha256, payload_version)`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow_array::{Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field};
use parangsae_core::{Amount, Qty, RunId, Symbol, UtcMicros};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::source::Source;
use crate::civil::{day_of, CivilDay};
use crate::store::{
    collect_i64, collect_string, collect_string_opt, collect_u32, field, i64s, read_batches,
    scale_schema, strings, u32s, write_batch,
};
use crate::vision::{KlineFact, TickDomain};
use crate::Error;

pub const PAYLOAD_V2: &str = "payload/v2";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredVersion {
    pub row: KlineFact,
    pub source: Source,
    pub data_hash: String,
    pub payload_version: String,
}

pub fn record_kline_archive(
    data_dir: &Path,
    dataset: &str,
    rows: Vec<KlineFact>,
    source: &Source,
) -> Result<crate::store::WriteCounts, Error> {
    let mut grouped: BTreeMap<CivilDay, Vec<KlineFact>> = BTreeMap::new();
    for row in rows {
        let day = day_of(row.open_time)?;
        grouped.entry(day).or_default().push(row);
    }
    let mut total = crate::store::WriteCounts::default();
    for (day, incoming) in grouped {
        append_versions(data_dir, dataset, day, &incoming, source)?;
        let symbol = incoming
            .first()
            .map(|row| row.symbol.as_str().to_owned())
            .unwrap_or_default();
        let path = crate::store::fact_path(data_dir, dataset, &symbol, day);
        let existing = if path.exists() {
            crate::store::load_klines(data_dir, dataset, &symbol, day)?
        } else {
            Vec::new()
        };
        let merged = crate::manifest::merge_rows(existing, incoming, |row| row.open_time.micros());
        if !merged.committed {
            for (stored, rejected) in &merged.conflicts {
                append_v2_line(data_dir, dataset, &symbol, stored, rejected)?;
            }
            let count = u64::try_from(merged.conflicts.len())
                .map_err(|_| Error::check("conflict", "conflict count"))?;
            total.conflicts += count;
            total.unchanged += merged.unchanged;
            continue;
        }
        if merged.inserted > 0 {
            crate::store::write_klines(&path, dataset, &merged.rows)?;
        }
        total.inserted += merged.inserted;
        total.unchanged += merged.unchanged;
    }
    Ok(total)
}

pub fn versions_on(
    data_dir: &Path,
    dataset: &str,
    symbol: &str,
    day: CivilDay,
) -> Result<Vec<StoredVersion>, Error> {
    let path = version_path(data_dir, dataset, symbol, day);
    if !path.exists() {
        return Ok(Vec::new());
    }
    read_versions(&path)
}

pub fn active_sha_hex(
    data_dir: &Path,
    dataset: &str,
    symbol: &str,
    pk_us: i64,
) -> Result<Vec<String>, Error> {
    let day = day_of(UtcMicros::from_micros(pk_us))?;
    let mut shas: Vec<String> = versions_on(data_dir, dataset, symbol, day)?
        .into_iter()
        .filter(|version| version.row.open_time.micros() == pk_us)
        .filter(|version| version.payload_version == PAYLOAD_V2)
        .map(|version| version.source.sha256_hex())
        .collect();
    shas.sort();
    shas.dedup();
    Ok(shas)
}

fn version_path(data_dir: &Path, dataset: &str, symbol: &str, day: CivilDay) -> PathBuf {
    data_dir
        .join("versions")
        .join(dataset)
        .join(format!("symbol={symbol}"))
        .join(format!("date={day}"))
        .join("part.parquet")
}

fn append_versions(
    data_dir: &Path,
    dataset: &str,
    day: CivilDay,
    rows: &[KlineFact],
    source: &Source,
) -> Result<(), Error> {
    let symbol = rows
        .first()
        .map(|row| row.symbol.as_str())
        .ok_or_else(|| Error::check("version_store", "no rows"))?;
    let path = version_path(data_dir, dataset, symbol, day);
    let mut stored = if path.exists() {
        read_versions(&path)?
    } else {
        Vec::new()
    };
    for row in rows {
        let hash = row.data_hash();
        if let Some(existing) = stored.iter().find(|version| {
            version.row.open_time.micros() == row.open_time.micros()
                && version.source.sha256 == source.sha256
                && version.payload_version == PAYLOAD_V2
        }) {
            if existing.data_hash != hash {
                return Err(Error::check(
                    "version_store_corrupt",
                    "same archive key and a different data_hash",
                ));
            }
            continue;
        }
        stored.push(StoredVersion {
            row: row.clone(),
            source: source.clone(),
            data_hash: hash,
            payload_version: PAYLOAD_V2.to_owned(),
        });
    }
    write_versions(&path, &stored)
}

fn write_versions(path: &Path, rows: &[StoredVersion]) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let schema = scale_schema(vec![
        field("symbol", DataType::Utf8),
        field("interval_us", DataType::Int64),
        field("open_time_us", DataType::Int64),
        field("close_time_us", DataType::Int64),
        field("open_ticks", DataType::Int64),
        field("high_ticks", DataType::Int64),
        field("low_ticks", DataType::Int64),
        field("close_ticks", DataType::Int64),
        field("volume_ticks", DataType::Int64),
        field("quote_volume_ticks", DataType::Int64),
        field("count", DataType::Int64),
        field("taker_buy_volume_ticks", DataType::Int64),
        field("taker_buy_quote_volume_ticks", DataType::Int64),
        field("tick_domain", DataType::Utf8),
        field("schema_version", DataType::UInt32),
        field("source_path", DataType::Utf8),
        field("source_sha256", DataType::Utf8),
        field("cadence", DataType::Utf8),
        Field::new("last_modified", DataType::Utf8, true),
        field("fetched_at_run", DataType::Utf8),
        field("data_hash", DataType::Utf8),
        field("payload_version", DataType::Utf8),
    ]);
    let last_modified: Vec<Option<String>> = rows
        .iter()
        .map(|version| version.source.last_modified.clone())
        .collect();
    let sha_hex: Vec<String> = rows
        .iter()
        .map(|version| version.source.sha256_hex())
        .collect();
    let runs: Vec<String> = rows
        .iter()
        .map(|version| version.source.fetched_at_run.to_hex())
        .collect();
    let columns: Vec<Arc<dyn Array>> = vec![
        strings(rows.iter().map(|version| version.row.symbol.as_str())),
        i64s(rows.iter().map(|version| version.row.interval_us)),
        i64s(rows.iter().map(|version| version.row.open_time.micros())),
        i64s(rows.iter().map(|version| version.row.close_time.micros())),
        i64s(rows.iter().map(|version| version.row.open_ticks)),
        i64s(rows.iter().map(|version| version.row.high_ticks)),
        i64s(rows.iter().map(|version| version.row.low_ticks)),
        i64s(rows.iter().map(|version| version.row.close_ticks)),
        i64s(rows.iter().map(|version| version.row.volume.ticks())),
        i64s(rows.iter().map(|version| version.row.quote_volume.ticks())),
        i64s(rows.iter().map(|version| version.row.count)),
        i64s(
            rows.iter()
                .map(|version| version.row.taker_buy_volume.ticks()),
        ),
        i64s(
            rows.iter()
                .map(|version| version.row.taker_buy_quote_volume.ticks()),
        ),
        strings(rows.iter().map(|version| version.row.tick_domain.as_str())),
        u32s(rows.iter().map(|version| version.row.schema_version)),
        strings(rows.iter().map(|version| version.source.path.as_str())),
        strings(sha_hex.iter().map(String::as_str)),
        strings(rows.iter().map(|version| version.source.cadence_str())),
        Arc::new(StringArray::from(last_modified)),
        strings(runs.iter().map(String::as_str)),
        strings(rows.iter().map(|version| version.data_hash.as_str())),
        strings(rows.iter().map(|version| version.payload_version.as_str())),
    ];
    let batch = RecordBatch::try_new(schema, columns)
        .map_err(|err| Error::check("version_store", err.to_string()))?;
    write_batch(path, batch)
}

fn read_versions(path: &Path) -> Result<Vec<StoredVersion>, Error> {
    let batches = read_batches(path)?;
    let symbol = collect_string(&batches, "symbol")?;
    let interval = collect_i64(&batches, "interval_us")?;
    let open_time = collect_i64(&batches, "open_time_us")?;
    let close_time = collect_i64(&batches, "close_time_us")?;
    let open_ticks = collect_i64(&batches, "open_ticks")?;
    let high = collect_i64(&batches, "high_ticks")?;
    let low = collect_i64(&batches, "low_ticks")?;
    let close_ticks = collect_i64(&batches, "close_ticks")?;
    let volume = collect_i64(&batches, "volume_ticks")?;
    let quote = collect_i64(&batches, "quote_volume_ticks")?;
    let count = collect_i64(&batches, "count")?;
    let taker = collect_i64(&batches, "taker_buy_volume_ticks")?;
    let taker_quote = collect_i64(&batches, "taker_buy_quote_volume_ticks")?;
    let domain = collect_string(&batches, "tick_domain")?;
    let schema_version = collect_u32(&batches, "schema_version")?;
    let source_path = collect_string(&batches, "source_path")?;
    let source_sha = collect_string(&batches, "source_sha256")?;
    let cadence = collect_string(&batches, "cadence")?;
    let last_modified = collect_string_opt(&batches, "last_modified")?;
    let run_id = collect_string(&batches, "fetched_at_run")?;
    let data_hash = collect_string(&batches, "data_hash")?;
    let payload_version = collect_string(&batches, "payload_version")?;
    let mut out = Vec::with_capacity(symbol.len());
    for index in 0..symbol.len() {
        let sha = parse_sha(&source_sha[index])?;
        let run = RunId::parse_hex(&run_id[index])
            .map_err(|err| Error::check("version_store", err.to_string()))?;
        let tick_domain = TickDomain::parse(&domain[index])
            .ok_or_else(|| Error::check("version_store", "tick domain"))?;
        let open = UtcMicros::from_micros(open_time[index]);
        let close = UtcMicros::from_micros(close_time[index]);
        let row = KlineFact {
            symbol: Symbol::new(&symbol[index]).map_err(Error::Symbol)?,
            interval_us: interval[index],
            open_time: open,
            close_time: close,
            open_ticks: open_ticks[index],
            high_ticks: high[index],
            low_ticks: low[index],
            close_ticks: close_ticks[index],
            volume: Qty::from_ticks(volume[index])?,
            quote_volume: Amount::from_ticks(quote[index])?,
            count: count[index],
            taker_buy_volume: Qty::from_ticks(taker[index])?,
            taker_buy_quote_volume: Amount::from_ticks(taker_quote[index])?,
            event_ts: parangsae_core::EventTs::from_micros(open),
            available_ts: parangsae_core::AvailableTs::from_micros(close),
            receipt: parangsae_core::Receipt::Archive {
                fetched_at_run: run,
            },
            schema_version: schema_version[index],
            tick_domain,
            zero_volume_flat: false,
        };
        let hash = row.data_hash();
        if hash != data_hash[index] {
            return Err(Error::check(
                "version_store_corrupt",
                "stored data_hash does not match the row",
            ));
        }
        out.push(StoredVersion {
            source: Source {
                path: source_path[index].clone(),
                sha256: sha,
                cadence: super::source::parse_cadence(&cadence[index])
                    .ok_or_else(|| Error::check("version_store", "cadence"))?,
                schema_version: schema_version[index],
                last_modified: last_modified[index].clone(),
                fetched_at_run: run,
            },
            data_hash: hash,
            payload_version: payload_version[index].clone(),
            row,
        });
    }
    Ok(out)
}

fn parse_sha(text: &str) -> Result<[u8; 32], Error> {
    let bytes = hex::decode(text).map_err(|err| Error::check("version_store", err.to_string()))?;
    let mut sha = [0u8; 32];
    if bytes.len() != 32 {
        return Err(Error::check("version_store", "sha256 length"));
    }
    sha.copy_from_slice(&bytes);
    Ok(sha)
}

#[derive(Serialize)]
struct SourceLine<'a> {
    path: &'a str,
    sha256: String,
    cadence: &'a str,
    schema_version: u32,
    last_modified: Option<&'a str>,
}

#[derive(Serialize)]
struct V2Line<'a> {
    v: u32,
    dataset: &'a str,
    symbol: &'a str,
    key: String,
    class: &'a str,
    stored: String,
    rejected: String,
    stored_sources: Vec<SourceLine<'a>>,
    rejected_sources: Vec<SourceLine<'a>>,
    version_set_sha256: Vec<String>,
    resolution_id: Option<&'a str>,
}

fn append_v2_line(
    data_dir: &Path,
    dataset: &str,
    symbol: &str,
    stored: &KlineFact,
    rejected: &KlineFact,
) -> Result<(), Error> {
    let day = day_of(stored.open_time)?;
    let versions = versions_on(data_dir, dataset, symbol, day)?;
    let pk = stored.open_time.micros();
    let at_key: Vec<&StoredVersion> = versions
        .iter()
        .filter(|version| version.row.open_time.micros() == pk)
        .filter(|version| version.payload_version == PAYLOAD_V2)
        .collect();
    let stored_hash = stored.data_hash();
    let rejected_hash = rejected.data_hash();
    let mut stored_sources: Vec<&StoredVersion> = at_key
        .iter()
        .copied()
        .filter(|version| version.data_hash == stored_hash)
        .collect();
    let mut rejected_sources: Vec<&StoredVersion> = at_key
        .iter()
        .copied()
        .filter(|version| version.data_hash == rejected_hash)
        .collect();
    stored_sources.sort_by(|left, right| left.source.rank_parts().cmp(&right.source.rank_parts()));
    rejected_sources
        .sort_by(|left, right| left.source.rank_parts().cmp(&right.source.rank_parts()));
    let mut version_set: Vec<String> = at_key
        .iter()
        .map(|version| version.source.sha256_hex())
        .collect();
    version_set.sort();
    version_set.dedup();
    let line = V2Line {
        v: 2,
        dataset,
        symbol,
        key: pk.to_string(),
        class: "value_by_log",
        stored: stored.data_payload_v2(),
        rejected: rejected.data_payload_v2(),
        stored_sources: stored_sources
            .iter()
            .map(|version| source_line(version))
            .collect(),
        rejected_sources: rejected_sources
            .iter()
            .map(|version| source_line(version))
            .collect(),
        version_set_sha256: version_set,
        resolution_id: None,
    };
    let text =
        serde_json::to_string(&line).map_err(|err| Error::check("conflict", err.to_string()))?;
    append_unique(data_dir, dataset, &text)
}

fn source_line(version: &StoredVersion) -> SourceLine<'_> {
    SourceLine {
        path: version.source.path.as_str(),
        sha256: version.source.sha256_hex(),
        cadence: version.source.cadence_str(),
        schema_version: version.source.schema_version,
        last_modified: version.source.last_modified.as_deref(),
    }
}

fn append_unique(data_dir: &Path, dataset: &str, line: &str) -> Result<(), Error> {
    let path = data_dir.join("conflicts").join(format!("{dataset}.jsonl"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let existing = if path.exists() {
        fs::read_to_string(&path).map_err(|source| Error::Io {
            path: path.display().to_string(),
            source,
        })?
    } else {
        String::new()
    };
    if existing.lines().any(|prior| same_v2(prior, line)) {
        return Ok(());
    }
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|source| Error::Io {
            path: path.display().to_string(),
            source,
        })?;
    writeln!(file, "{line}").map_err(|source| Error::Io {
        path: path.display().to_string(),
        source,
    })
}

pub struct LoggedConflict {
    pub pk: i64,
    pub class: String,
    pub stored: String,
    pub rejected: String,
}

pub fn logged_conflicts(
    data_dir: &Path,
    dataset: &str,
    symbol: &str,
    day: CivilDay,
) -> Result<Vec<LoggedConflict>, Error> {
    let path = data_dir.join("conflicts").join(format!("{dataset}.jsonl"));
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).map_err(|source| Error::Io {
        path: path.display().to_string(),
        source,
    })?;
    let mut by_key: BTreeMap<i64, Vec<serde_json::Value>> = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let parsed: serde_json::Value = serde_json::from_str(line)
            .map_err(|err| Error::check("conflict", format!("line {}: {err}", index + 1)))?;
        if parsed.get("symbol").and_then(|value| value.as_str()) != Some(symbol) {
            continue;
        }
        let key = parsed
            .get("key")
            .and_then(|value| value.as_str())
            .ok_or_else(|| Error::check("conflict", format!("line {} has no key", index + 1)))?;
        let pk = key
            .parse::<i64>()
            .map_err(|_| Error::check("conflict", format!("line {} key", index + 1)))?;
        let row_day = day_of(UtcMicros::from_micros(pk))?;
        if row_day != day {
            continue;
        }
        by_key.entry(pk).or_default().push(parsed);
    }
    let mut out = Vec::new();
    for (pk, lines) in by_key {
        let active = active_sha_hex(data_dir, dataset, symbol, pk)?;
        let mut current_v2 = Vec::new();
        let mut legacy = Vec::new();
        for line in lines {
            if line.get("v").and_then(|value| value.as_u64()) == Some(2) {
                let mut set = string_list(line.get("version_set_sha256"));
                set.sort();
                if set == active {
                    current_v2.push(line);
                }
            } else {
                legacy.push(line);
            }
        }
        let chosen = if current_v2.is_empty() {
            legacy
        } else {
            current_v2
        };
        for line in chosen {
            let stored = json_text(&line, "stored")?;
            let rejected = json_text(&line, "rejected")?;
            let class = if line.get("v").and_then(|value| value.as_u64()) == Some(2) {
                json_text(&line, "class")?
            } else {
                legacy_class(&stored, &rejected).to_owned()
            };
            out.push(LoggedConflict {
                pk,
                class,
                stored,
                rejected,
            });
        }
    }
    Ok(out)
}

fn string_list(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|item| item.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn json_text(value: &serde_json::Value, key: &str) -> Result<String, Error> {
    value
        .get(key)
        .and_then(|item| item.as_str())
        .map(str::to_owned)
        .ok_or_else(|| Error::check("conflict", format!("missing {key}")))
}

fn legacy_class(stored: &str, rejected: &str) -> &'static str {
    let left: Vec<&str> = stored
        .split(' ')
        .filter(|token| !token.starts_with("schema="))
        .collect();
    let right: Vec<&str> = rejected
        .split(' ')
        .filter(|token| !token.starts_with("schema="))
        .collect();
    if left == right {
        "schema_only_by_log"
    } else {
        "value_by_log"
    }
}

fn same_v2(prior: &str, line: &str) -> bool {
    let Ok(left) = serde_json::from_str::<serde_json::Value>(prior) else {
        return false;
    };
    let Ok(right) = serde_json::from_str::<serde_json::Value>(line) else {
        return false;
    };
    let stored_sha = |value: &serde_json::Value, key: &str| {
        value
            .get(key)
            .and_then(|item| item.as_str())
            .map(|text| hex::encode(Sha256::digest(text.as_bytes())))
            .unwrap_or_default()
    };
    left.get("dataset") == right.get("dataset")
        && left.get("symbol") == right.get("symbol")
        && left.get("key") == right.get("key")
        && left.get("class") == right.get("class")
        && stored_sha(&left, "stored") == stored_sha(&right, "stored")
        && stored_sha(&left, "rejected") == stored_sha(&right, "rejected")
        && left.get("version_set_sha256") == right.get("version_set_sha256")
}
