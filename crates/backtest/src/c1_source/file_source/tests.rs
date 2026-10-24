use super::*;
use arrow_array::ArrayRef;
use arrow_schema::{Field, Schema};
use cucumber::{given, then, when, World as _};
use parquet::arrow::ArrowWriter;
use std::{collections::HashMap, fs::Permissions, os::unix::fs::PermissionsExt, sync::Arc};

fn price_fixture(root: &Path, day: i64, keys: &[i64], poison: bool) -> BoundObject {
    let mut fields = Vec::new();
    let mut columns: Vec<ArrayRef> = Vec::new();
    for name in KLINE_NAMES {
        let data: ArrayRef = match name {
            "symbol" => Arc::new(StringArray::from(vec!["BTCUSDT"; keys.len()])),
            "tick_domain" => Arc::new(StringArray::from(vec!["price"; keys.len()])),
            "schema_version" => Arc::new(UInt32Array::from(vec![1; keys.len()])),
            _ => Arc::new(Int64Array::from(
                keys.iter()
                    .map(|key| match name {
                        "open_time_us" | "event_ts_us" => *key,
                        "close_time_us" => key + MINUTE - if day >= SPOT_MICROS { 1 } else { 1000 },
                        "available_ts_us" => source_available(PriceSource::Spot, *key).unwrap(),
                        "received_ts_us" => END + DAY,
                        "interval_us" => MINUTE,
                        "count" => 1,
                        "open_ticks" if poison => -1,
                        "open_ticks" | "high_ticks" | "low_ticks" | "close_ticks" => 100_000_000,
                        "volume_ticks" | "quote_volume_ticks" => 100_000_000,
                        _ => 0,
                    })
                    .collect::<Vec<_>>(),
            )),
        };
        fields.push(Field::new(name, data.data_type().clone(), false));
        columns.push(data);
    }
    let schema = Arc::new(Schema::new_with_metadata(
        fields,
        HashMap::from([("parangsae.scale".into(), "100000000".into())]),
    ));
    let path = root.canonicalize().unwrap().join("synthetic.parquet");
    let mut writer =
        ArrowWriter::try_new(File::create(&path).unwrap(), schema.clone(), None).unwrap();
    writer
        .write(&RecordBatch::try_new(schema, columns).unwrap())
        .unwrap();
    writer.close().unwrap();
    fs::set_permissions(&path, Permissions::from_mode(0o444)).unwrap();
    let sha256 = Sha256::digest(fs::read(&path).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    BoundObject {
        object: Object {
            path: "synthetic.parquet".into(),
            sha256,
            role: Role::Spot,
            date: UtcDate::from_micros(UtcMicros::from_micros(day)).to_string(),
        },
        before: Identity::from(&regular(&path).unwrap()),
        path,
    }
}

fn status_fixture(root: &Path, day: i64, rows: &[serde_json::Value]) -> BoundObject {
    let path = root.canonicalize().unwrap().join("status.jsonl");
    fs::write(
        &path,
        rows.iter()
            .map(|row| format!("{row}\n"))
            .collect::<String>(),
    )
    .unwrap();
    fs::set_permissions(&path, Permissions::from_mode(0o444)).unwrap();
    let sha256 = Sha256::digest(fs::read(&path).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    BoundObject {
        object: Object {
            path: "status.jsonl".into(),
            sha256,
            role: Role::SpotStatus,
            date: UtcDate::from_micros(UtcMicros::from_micros(day)).to_string(),
        },
        before: Identity::from(&regular(&path).unwrap()),
        path,
    }
}

#[test]
fn real_funding_parquet_preserves_raw_event_and_applies_floor_once() {
    let root = tempfile::tempdir().unwrap();
    let mut fields = Vec::new();
    let mut columns: Vec<ArrayRef> = Vec::new();
    for name in FUNDING_NAMES {
        let column: ArrayRef = match name {
            "symbol" => Arc::new(StringArray::from(vec!["BTCUSDT"; 3])),
            "schema_version" => Arc::new(UInt32Array::from(vec![3; 3])),
            _ => Arc::new(Int64Array::from(
                (0..3)
                    .map(|i| {
                        let key = SAMPLE + i * SLOT;
                        match name {
                            "funding_time_us" => key,
                            "raw_calc_time_us" | "event_ts_us" | "available_ts_us" => key + 5000,
                            "received_ts_us" => END + DAY,
                            "interval_hours" => 8,
                            _ => 0,
                        }
                    })
                    .collect::<Vec<_>>(),
            )),
        };
        fields.push(Field::new(name, column.data_type().clone(), false));
        columns.push(column);
    }
    let schema = Arc::new(Schema::new_with_metadata(
        fields,
        HashMap::from([("parangsae.scale".into(), "100000000".into())]),
    ));
    let path = root.path().canonicalize().unwrap().join("funding.parquet");
    let mut writer =
        ArrowWriter::try_new(File::create(&path).unwrap(), schema.clone(), None).unwrap();
    writer
        .write(&RecordBatch::try_new(schema, columns).unwrap())
        .unwrap();
    writer.close().unwrap();
    fs::set_permissions(&path, Permissions::from_mode(0o444)).unwrap();
    let sha256 = Sha256::digest(fs::read(&path).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let object = BoundObject {
        object: Object {
            path: "funding.parquet".into(),
            sha256,
            role: Role::Funding,
            date: "2020-12-18".into(),
        },
        before: Identity::from(&regular(&path).unwrap()),
        path,
    };
    let rows = funding_rows(&object, SAMPLE).unwrap();
    assert_eq!(rows.len(), 3);
    let first = &rows[&SAMPLE];
    let admitted = admitted_funding(
        first,
        DecisionTime::from_micros(UtcMicros::from_micros(SAMPLE + MINUTE)),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        admitted.source.original.event_ts.micros().micros(),
        SAMPLE + 5000
    );
    assert_eq!(
        admitted.source.original.received_ts.micros().micros(),
        END + DAY
    );
    assert_eq!(
        admitted.observation.timestamps().event_ts,
        admitted.source.original.event_ts
    );
    assert_eq!(
        admitted
            .observation
            .timestamps()
            .available_ts
            .micros()
            .micros(),
        SAMPLE + MINUTE
    );
    assert_eq!(
        admitted
            .observation
            .timestamps()
            .received_ts
            .micros()
            .micros(),
        SAMPLE + MINUTE
    );
}

#[derive(cucumber::World, Debug, Default)]
struct FileWorld {
    fixture: Option<(tempfile::TempDir, FileSource)>,
    refusal: Option<SourceError>,
}

#[given("an incomplete C1 source file with an invalid price payload")]
fn incomplete_source(world: &mut FileWorld) {
    let root = tempfile::tempdir().unwrap();
    let object = price_fixture(root.path(), SAMPLE, &[SAMPLE], true);
    let source = FileSource {
        objects: BTreeMap::from([((Role::Spot, SAMPLE), object)]),
        price_cache: RefCell::new(VecDeque::new()),
        funding_cache: RefCell::new(VecDeque::new()),
    };
    world.fixture = Some((root, source));
}

#[when("the file reader admits the declared C1 day")]
fn admit_file(world: &mut FileWorld) {
    world.refusal = world
        .fixture
        .as_ref()
        .unwrap()
        .1
        .day(PriceSource::Spot, SAMPLE)
        .err();
}

#[then("key coverage is refused before the price payload and no day is admitted")]
fn file_refused(world: &mut FileWorld) {
    assert!(
        matches!(&world.refusal, Some(SourceError::Input { reason, .. }) if reason=="source key set before payload")
    );
    assert!(world
        .fixture
        .as_ref()
        .unwrap()
        .1
        .price_cache
        .borrow()
        .is_empty());
}

#[tokio::test]
async fn file_source_feature() {
    FileWorld::cucumber()
        .with_default_cli()
        .fail_on_skipped()
        .run_and_exit(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/features/c1_file_source.feature"
        ))
        .await;
}

#[test]
fn file_statuses_partition_missing_and_quarantined_keys_without_a_price() {
    let root = tempfile::tempdir().unwrap();
    let day = UtcDate::parse("2021-12-24").unwrap().start_us().micros();
    let keys: Vec<_> = (2..1440).map(|i| day + i * MINUTE).collect();
    let price = price_fixture(root.path(), day, &keys, false);
    let statuses = [
        serde_json::json!({"kind":"Gap","open_time_us":day,"modeled_notice_at_us":day+MINUTE,"reason":"absent selected original source key","source":{"object":"synthetic","row":null}}),
        serde_json::json!({"kind":"Quarantined","open_time_us":day+MINUTE,"modeled_notice_at_us":day+2*MINUTE,"reason":"unsupported original close","source":{"object":"synthetic","row":2},"raw_close":"original-bad-close"}),
    ];
    let status = status_fixture(root.path(), day, &statuses);
    let source = FileSource {
        objects: BTreeMap::from([
            ((Role::Spot, day), price),
            ((Role::SpotStatus, day), status),
        ]),
        price_cache: RefCell::new(VecDeque::new()),
        funding_cache: RefCell::new(VecDeque::new()),
    };
    assert!(matches!(
        source.bar(PriceSource::Spot, day),
        Err(SourceError::Unavailable {
            health: SourceHealth::Gap,
            ..
        })
    ));
    assert!(matches!(
        source.bar(PriceSource::Spot, day + MINUTE),
        Err(SourceError::Unavailable {
            health: SourceHealth::Quarantined,
            ..
        })
    ));
    assert_eq!(
        source
            .bar(PriceSource::Spot, day + 2 * MINUTE)
            .unwrap()
            .bar
            .open_time
            .micros(),
        day + 2 * MINUTE
    );
    let cached = source.price_cache.borrow();
    let rows = &cached.front().unwrap().1;
    assert_eq!(rows.bars.len(), 1438);
    assert_eq!(rows.status.len(), 2);
}

#[test]
fn price_free_status_cannot_be_duplicate_or_notice_early() {
    let root = tempfile::tempdir().unwrap();
    let row = serde_json::json!({"kind":"Gap","open_time_us":SAMPLE,"modeled_notice_at_us":SAMPLE+MINUTE-1000,"reason":"absent source","source":{"object":"synthetic"}});
    let object = status_fixture(root.path(), SAMPLE, &[row]);
    assert!(status_rows(&object, SAMPLE).is_err());
    fs::remove_file(&object.path).unwrap();
    let row = serde_json::json!({"kind":"Gap","open_time_us":SAMPLE,"modeled_notice_at_us":SAMPLE+MINUTE,"reason":"absent source","source":{"object":"synthetic"}});
    let object = status_fixture(root.path(), SAMPLE, &[row.clone(), row]);
    assert!(status_rows(&object, SAMPLE).is_err());
}

#[test]
fn normal_sparse_status_is_only_the_exact_certified_gap_key_set() {
    for (date, start, count) in CERTIFIED_NORMAL_GAPS {
        let root = tempfile::tempdir().unwrap();
        let day = UtcDate::parse(date).unwrap().start_us().micros();
        let mut rows: Vec<_> = (0..count).map(|i| {
            let key = start+i*MINUTE;
            serde_json::json!({"kind":"Gap","open_time_us":key,"modeled_notice_at_us":key+MINUTE,"reason":"same original admitted source key census","source":{"object":"synthetic-original","row":null}})
        }).collect();
        let object = status_fixture(root.path(), day, &rows);
        assert_eq!(
            status_rows(&object, day).unwrap().len(),
            usize::try_from(count).unwrap()
        );
        fs::remove_file(&object.path).unwrap();
        rows.pop();
        let object = status_fixture(root.path(), day, &rows);
        assert!(
            matches!(status_rows(&object,day),Err(SourceError::Input { reason,.. }) if reason=="normal status differs from certified original-source key set")
        );
    }
}

#[test]
fn incomplete_key_set_is_refused_before_poisoned_payload() {
    let root = tempfile::tempdir().unwrap();
    let object = price_fixture(root.path(), SAMPLE, &[SAMPLE], true);
    let source = FileSource {
        objects: BTreeMap::from([((Role::Spot, SAMPLE), object)]),
        price_cache: RefCell::new(VecDeque::new()),
        funding_cache: RefCell::new(VecDeque::new()),
    };
    let error = source.day(PriceSource::Spot, SAMPLE).unwrap_err();
    assert!(
        matches!(&error, SourceError::Input { reason, .. } if reason == "source key set before payload"),
        "{error:?}"
    );
}

#[test]
fn duplicate_key_is_refused_before_poisoned_payload() {
    let root = tempfile::tempdir().unwrap();
    let object = price_fixture(root.path(), SAMPLE, &[SAMPLE, SAMPLE], true);
    let error = keys(&object, SAMPLE, false, false).unwrap_err();
    assert!(
        matches!(error, SourceError::Input { reason, .. } if reason == "symbol/calendar/grid/duplicate key before payload")
    );
}

#[test]
fn oversized_day_is_refused_from_metadata_before_reading_key_batches() {
    let root = tempfile::tempdir().unwrap();
    let object = price_fixture(root.path(), SAMPLE, &[SAMPLE; 1441], true);
    assert!(
        matches!(keys(&object,SAMPLE,false,false),Err(SourceError::Input { reason,.. }) if reason=="daily row capacity")
    );
}

#[test]
fn real_parquet_preserves_original_receipt_and_post2025_clock() {
    let root = tempfile::tempdir().unwrap();
    let keys: Vec<_> = (0..1440).map(|i| SPOT_MICROS + i * MINUTE).collect();
    let object = price_fixture(root.path(), SPOT_MICROS, &keys, false);
    let source = FileSource {
        objects: BTreeMap::from([((Role::Spot, SPOT_MICROS), object)]),
        price_cache: RefCell::new(VecDeque::new()),
        funding_cache: RefCell::new(VecDeque::new()),
    };
    let row = source.bar(PriceSource::Spot, SPOT_MICROS).unwrap();
    assert_eq!(row.bar.close_time.micros(), SPOT_MICROS + MINUTE - 1);
    assert_eq!(
        row.source.original.available_ts.micros().micros(),
        SPOT_MICROS + MINUTE + 999
    );
    assert_eq!(row.source.original.received_ts.micros().micros(), END + DAY);
    let view = C1SourceView {
        bars: BTreeMap::new(),
        statuses: BTreeMap::new(),
        funding: BTreeMap::new(),
        files: Some(source),
    };
    assert!(matches!(
        view.execution_at(
            C1Instrument::Spot,
            UtcMicros::from_micros(SPOT_MICROS),
            DecisionTime::from_micros(UtcMicros::from_micros(SPOT_MICROS + MINUTE))
        )
        .unwrap(),
        ExecutionState::Pending { .. }
    ));
    let ExecutionState::Valid {
        timestamps, source, ..
    } = view
        .execution_at(
            C1Instrument::Spot,
            UtcMicros::from_micros(SPOT_MICROS),
            DecisionTime::from_micros(UtcMicros::from_micros(SPOT_MICROS + MINUTE + 1000)),
        )
        .unwrap()
    else {
        panic!("visible file-backed evidence required")
    };
    assert_eq!(
        timestamps.received_ts.micros().micros(),
        SPOT_MICROS + MINUTE + 1000
    );
    assert_eq!(source.original.received_ts.micros().micros(), END + DAY);
}

#[test]
fn file_identity_hash_and_independent_readonly_contract_can_refuse() {
    let root = tempfile::tempdir().unwrap();
    let mut object = price_fixture(root.path(), SAMPLE, &[SAMPLE], false);
    object.object.sha256 = "0".repeat(64);
    assert!(
        matches!(object.open(), Err(SourceError::Input { reason, .. }) if reason == "input digest mismatch")
    );
    fs::set_permissions(&object.path, Permissions::from_mode(0o644)).unwrap();
    assert!(regular(&object.path).is_err());
    fs::set_permissions(&object.path, Permissions::from_mode(0o444)).unwrap();
    let alias = root.path().join("hardlink.parquet");
    fs::hard_link(&object.path, alias).unwrap();
    assert!(regular(&object.path).is_err());
}

#[test]
fn forbidden_or_escaped_logical_paths_are_rejected_without_opening_them() {
    for path in [
        "/never-open",
        "../never-open",
        "parquet/spot-kline/symbol=BTCUSDT/date=2025-10-01/part.parquet",
    ] {
        let object = Object {
            path: path.into(),
            sha256: "0".repeat(64),
            role: Role::Spot,
            date: "2025-10-01".into(),
        };
        assert!(logical(&object).is_err());
    }
}

#[test]
fn source_manifest_refuses_a_holdout_filename_before_any_data_file_exists() {
    let root = tempfile::tempdir().unwrap();
    let path = root
        .path()
        .canonicalize()
        .unwrap()
        .join("c1-source-manifest.json");
    let manifest = serde_json::json!({
        "schema":"parangsae-c1-source-manifest-v1", "symbol":"BTCUSDT",
        "source_start_us":SAMPLE,"source_end_us_exclusive":END,
        "source_policy_id":"C1_ARCHIVED_SPARSE_KEY_CERTIFIED_V2",
        "daily_rebuilt_days":REBUILT,"spot_status_days":STATUS_DAYS,
        "data_objects":[{"path":"parquet/spot-kline/symbol=BTCUSDT/date=2025-10-01/part.parquet","sha256":"0".repeat(64),"role":"spot-trade","date":"2025-10-01"}]
    });
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    fs::set_permissions(&path, Permissions::from_mode(0o444)).unwrap();
    assert!(
        matches!(C1SourceView::load_exact(&path), Err(SourceError::Input { reason, .. }) if reason=="forbidden source date")
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn every_settlement_must_support_the_first_profile_even_without_a_position() {
    let root = tempfile::tempdir().unwrap();
    let object = price_fixture(root.path(), SAMPLE, &[SAMPLE], false);
    let mut proxy = price_rows(
        &object,
        PriceSource::UmMark,
        SAMPLE,
        false,
        &BTreeSet::from([SAMPLE]),
    )
    .unwrap()
    .remove(&SAMPLE)
    .unwrap();
    let mut row = SourceFunding {
        settlement: UtcMicros::from_micros(SAMPLE),
        rate: Ratio::from_ticks(0),
        interval_hours: 8,
        source: proxy.source.clone(),
    };
    assert!(cash_profile(&row, &proxy).is_ok());
    row.source.original.available_ts =
        AvailableTs::from_micros(UtcMicros::from_micros(SAMPLE + MINUTE + 1));
    assert!(
        matches!(cash_profile(&row, &proxy), Err(SourceError::Input { reason, .. }) if reason == "unsupported funding availability at S+60s")
    );
    row.source.original.available_ts = AvailableTs::from_micros(UtcMicros::from_micros(SAMPLE));
    proxy.bar.open = Price::from_ticks(0).unwrap();
    assert!(cash_profile(&row, &proxy).is_err());
}
