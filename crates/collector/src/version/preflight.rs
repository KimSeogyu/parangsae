//! Fold preflight. A failing check stamps that fold and the run continues.

use std::collections::BTreeMap;

use crate::version::id::DatasetId;
use crate::version::load::{ExcludedWindow, GapEntry, Table, Verified};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Requirement {
    Required,
    Optional,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RequirementError {
    CannotRelaxDefault { dataset: DatasetId },
    MalformedSymbol,
}

impl std::fmt::Display for RequirementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CannotRelaxDefault { dataset } => {
                write!(f, "cannot relax required dataset {dataset}")
            }
            Self::MalformedSymbol => write!(f, "symbol is not a dataset-id segment"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldSpec {
    pub fold_id: String,
    pub warmup_start_us: i64,
    pub window_start_us: i64,
    pub window_end_us: i64,
    pub warmup_rows: BTreeMap<DatasetId, u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreflightError {
    MissingRequiredDataset {
        dataset: DatasetId,
    },
    GapInRequired {
        dataset: DatasetId,
        kind: String,
        start_us: i64,
        end_us: i64,
    },
    InsufficientWarmup {
        dataset: DatasetId,
        have_rows: u64,
        need_rows: u64,
    },
}

impl PreflightError {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::MissingRequiredDataset { .. } => "MissingRequiredDataset",
            Self::GapInRequired { .. } => "GapInRequired",
            Self::InsufficientWarmup { .. } => "InsufficientWarmup",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Degradation {
    pub dataset: DatasetId,
    pub kind: String,
    pub start_us: i64,
    pub end_us: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldStamp {
    pub fold_id: String,
    pub excluded_windows: Vec<ExcludedWindow>,
    pub degraded: Vec<Degradation>,
    pub data_abort: Option<PreflightError>,
}

/// Defaults: um-kline, funding-rate, and mark-price-kline are required.
/// book-depth is optional. `None` on a declared id means required.
pub fn requirements(
    symbol: &str,
    declared: &[(DatasetId, Option<Requirement>)],
) -> Result<BTreeMap<DatasetId, Requirement>, RequirementError> {
    let mut map = BTreeMap::new();
    for name in ["um-kline", "funding-rate", "mark-price-kline"] {
        map.insert(dataset_id(name, symbol)?, Requirement::Required);
    }
    map.insert(dataset_id("book-depth", symbol)?, Requirement::Optional);
    for (id, requirement) in declared {
        match requirement {
            None => {
                map.insert(id.clone(), Requirement::Required);
            }
            Some(Requirement::Required) => {
                map.insert(id.clone(), Requirement::Required);
            }
            Some(Requirement::Optional) if locked(id) => {
                return Err(RequirementError::CannotRelaxDefault {
                    dataset: id.clone(),
                });
            }
            Some(Requirement::Optional) => {
                map.insert(id.clone(), Requirement::Optional);
            }
        }
    }
    Ok(map)
}

pub fn preflight(
    required: &BTreeMap<DatasetId, Requirement>,
    tables: &BTreeMap<DatasetId, &Verified<Table>>,
    folds: &[FoldSpec],
) -> Vec<FoldStamp> {
    folds
        .iter()
        .map(|fold| stamp_fold(required, tables, fold))
        .collect()
}

fn stamp_fold(
    required: &BTreeMap<DatasetId, Requirement>,
    tables: &BTreeMap<DatasetId, &Verified<Table>>,
    fold: &FoldSpec,
) -> FoldStamp {
    let missing = required.iter().find_map(|(id, requirement)| {
        if *requirement == Requirement::Required && !tables.contains_key(id) {
            Some(id.clone())
        } else {
            None
        }
    });
    let mut degraded = Vec::new();
    let mut aborting: Option<PreflightError> = None;
    for (id, table) in tables {
        let requirement = required.get(id).copied().unwrap_or(Requirement::Optional);
        for gap in table.open_gaps() {
            if !intersects(
                gap.start_us,
                gap.end_us,
                fold.warmup_start_us,
                fold.window_end_us,
            ) {
                continue;
            }
            if gap.kind == "allowlisted_missing" || requirement == Requirement::Optional {
                degraded.push(Degradation {
                    dataset: id.clone(),
                    kind: degradation_kind(id, &gap.kind),
                    start_us: gap.start_us,
                    end_us: gap.end_us,
                });
                continue;
            }
            if aborting.is_none() || earlier_than(gap, id, aborting.as_ref()) {
                aborting = Some(PreflightError::GapInRequired {
                    dataset: id.clone(),
                    kind: gap.kind.clone(),
                    start_us: gap.start_us,
                    end_us: gap.end_us,
                });
            }
        }
    }
    let excluded = intersecting_windows(tables, fold);
    let data_abort = if let Some(dataset) = missing {
        Some(PreflightError::MissingRequiredDataset { dataset })
    } else if let Some(gap) = aborting {
        Some(gap)
    } else {
        warmup_abort(tables, fold)
    };
    FoldStamp {
        fold_id: fold.fold_id.clone(),
        excluded_windows: excluded,
        degraded,
        data_abort,
    }
}

fn earlier_than(gap: &GapEntry, id: &DatasetId, current: Option<&PreflightError>) -> bool {
    let Some(PreflightError::GapInRequired {
        dataset,
        start_us,
        end_us,
        ..
    }) = current
    else {
        return true;
    };
    (gap.start_us, gap.end_us, id) < (*start_us, *end_us, dataset)
}

fn intersecting_windows(
    tables: &BTreeMap<DatasetId, &Verified<Table>>,
    fold: &FoldSpec,
) -> Vec<ExcludedWindow> {
    let mut windows = Vec::new();
    for table in tables.values() {
        for window in table.excluded_windows() {
            if intersects(
                window.avail_start_us,
                window.avail_end_us,
                fold.warmup_start_us,
                fold.window_end_us,
            ) {
                windows.push(window.clone());
            }
        }
    }
    windows
}

/// A declared minimum applies to every loaded dataset, including Optional.
/// An Optional id that was not loaded is not an abort: only Required ids must
/// be present.
fn warmup_abort(
    tables: &BTreeMap<DatasetId, &Verified<Table>>,
    fold: &FoldSpec,
) -> Option<PreflightError> {
    for (id, need) in &fold.warmup_rows {
        let Some(table) = tables.get(id) else {
            continue;
        };
        let have = warmup_rows(table, fold);
        if have < *need {
            return Some(PreflightError::InsufficientWarmup {
                dataset: id.clone(),
                have_rows: have,
                need_rows: *need,
            });
        }
    }
    None
}

fn warmup_rows(table: &Verified<Table>, fold: &FoldSpec) -> u64 {
    let mut count = 0u64;
    for (pk, available) in row_clocks(table.value()) {
        if available >= fold.warmup_start_us
            && available < fold.window_start_us
            && !table.is_excluded(pk)
        {
            count += 1;
        }
    }
    count
}

fn row_clocks(table: &Table) -> Vec<(i64, i64)> {
    match table {
        Table::Kline(rows) => rows
            .iter()
            .map(|row| (row.open_time.micros(), row.available_ts.micros().micros()))
            .collect(),
        Table::Funding(rows) => rows
            .iter()
            .map(|row| {
                (
                    row.funding_time.micros(),
                    row.available_ts.micros().micros(),
                )
            })
            .collect(),
        Table::Metrics(rows) => rows
            .iter()
            .map(|row| (row.create_time.micros(), row.available_ts.micros().micros()))
            .collect(),
        Table::Book(rows) => rows
            .iter()
            .map(|row| (row.timestamp.micros(), row.available_ts.micros().micros()))
            .collect(),
    }
}

fn intersects(start: i64, end: i64, warmup_start: i64, window_end: i64) -> bool {
    if start == end {
        warmup_start <= start && start < window_end
    } else {
        start < window_end && end > warmup_start
    }
}

fn degradation_kind(id: &DatasetId, gap_kind: &str) -> String {
    if id.dataset() == "book-depth" {
        "fixed_bps".to_owned()
    } else {
        gap_kind.to_owned()
    }
}

fn locked(id: &DatasetId) -> bool {
    matches!(
        id.dataset(),
        "um-kline" | "funding-rate" | "mark-price-kline"
    )
}

fn dataset_id(dataset: &str, symbol: &str) -> Result<DatasetId, RequirementError> {
    DatasetId::parse(&format!("binance-vision.um.{dataset}.{symbol}"))
        .map_err(|_| RequirementError::MalformedSymbol)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use parangsae_core::{Receipt, RunId, Symbol};

    use super::*;
    use crate::civil::CivilDay;
    use crate::normalize::UM_KLINE_CLOCK;
    use crate::vision::{parse_klines, KlineFact, TickDomain};

    fn show(err: impl std::fmt::Display) -> String {
        err.to_string()
    }

    fn id(dataset: &str) -> Result<DatasetId, String> {
        DatasetId::parse(&format!("binance-vision.um.{dataset}.BTCUSDT")).map_err(show)
    }

    fn gap(kind: &str, start: i64, end: i64) -> GapEntry {
        GapEntry {
            kind: kind.to_owned(),
            start_us: start,
            end_us: end,
        }
    }

    fn fold(
        fold_id: &str,
        warmup_start_us: i64,
        window_start_us: i64,
        window_end_us: i64,
        warmup_rows: BTreeMap<DatasetId, u64>,
    ) -> FoldSpec {
        FoldSpec {
            fold_id: fold_id.to_owned(),
            warmup_start_us,
            window_start_us,
            window_end_us,
            warmup_rows,
        }
    }

    fn map_one(
        id: DatasetId,
        requirement: Requirement,
        table: &Verified<Table>,
    ) -> (
        BTreeMap<DatasetId, Requirement>,
        BTreeMap<DatasetId, &Verified<Table>>,
    ) {
        let mut required = BTreeMap::new();
        required.insert(id.clone(), requirement);
        let mut tables = BTreeMap::new();
        tables.insert(id, table);
        (required, tables)
    }

    fn day_us(year: i32, month: u8, day: u8) -> Result<i64, String> {
        CivilDay::new(year, month, day)
            .map(|day| day.start_us().micros())
            .ok_or_else(|| format!("civil day {year}-{month}-{day}"))
    }

    fn kline_rows() -> Result<Vec<KlineFact>, String> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/vision/um_kline_2021-06-01_headerless.csv");
        let text = fs::read_to_string(&path).map_err(show)?;
        let parsed = parse_klines(
            &text,
            &Symbol::new("BTCUSDT").map_err(show)?,
            UM_KLINE_CLOCK,
            TickDomain::Price,
            Receipt::Archive {
                fetched_at_run: RunId::from_bytes([1; 16]),
            },
            true,
        )
        .map_err(show)?;
        if parsed.rows.len() < 3 {
            return Err("fixture has fewer than 3 rows".to_owned());
        }
        Ok(parsed.rows)
    }

    fn pk(row: &KlineFact) -> i64 {
        row.open_time.micros()
    }

    fn avail(row: &KlineFact) -> i64 {
        row.available_ts.micros().micros()
    }

    #[test]
    fn requirement_map_locks_defaults_and_a_missing_id_aborts_every_fold() -> Result<(), String> {
        let map = requirements("BTCUSDT", &[]).map_err(show)?;
        assert_eq!(map.get(&id("um-kline")?), Some(&Requirement::Required));
        assert_eq!(map.get(&id("funding-rate")?), Some(&Requirement::Required));
        assert_eq!(
            map.get(&id("mark-price-kline")?),
            Some(&Requirement::Required)
        );
        assert_eq!(map.get(&id("book-depth")?), Some(&Requirement::Optional));

        let metrics = id("metrics")?;
        let unspecified = requirements("BTCUSDT", &[(metrics.clone(), None)]).map_err(show)?;
        assert_eq!(unspecified.get(&metrics), Some(&Requirement::Required));
        let optional = requirements("BTCUSDT", &[(metrics.clone(), Some(Requirement::Optional))])
            .map_err(show)?;
        assert_eq!(optional.get(&metrics), Some(&Requirement::Optional));
        let forced =
            requirements("BTCUSDT", &[(metrics, Some(Requirement::Required))]).map_err(show)?;
        assert_eq!(forced.get(&id("metrics")?), Some(&Requirement::Required));

        for name in ["um-kline", "funding-rate", "mark-price-kline"] {
            let locked = id(name)?;
            match requirements("BTCUSDT", &[(locked.clone(), Some(Requirement::Optional))]) {
                Err(RequirementError::CannotRelaxDefault { dataset }) => {
                    assert_eq!(dataset, locked);
                }
                Err(RequirementError::MalformedSymbol) => {
                    return Err("expected CannotRelaxDefault".to_owned());
                }
                Ok(_) => return Err(format!("{name} optional override was accepted")),
            }
        }

        match requirements("", &[]) {
            Err(RequirementError::MalformedSymbol) => {}
            other => return Err(format!("empty symbol: {other:?}")),
        }
        match requirements("BTC.USDT", &[]) {
            Err(RequirementError::MalformedSymbol) => {}
            other => return Err(format!("dotted symbol: {other:?}")),
        }

        let funding = id("funding-rate")?;
        let tables = BTreeMap::new();
        let folds = [
            fold("a", 0, 0, 1, BTreeMap::new()),
            fold("b", 10, 10, 20, BTreeMap::new()),
        ];
        let stamps = preflight(&map, &tables, &folds);
        assert_eq!(
            stamps
                .iter()
                .map(|stamp| stamp.fold_id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        for stamp in &stamps {
            match &stamp.data_abort {
                Some(PreflightError::MissingRequiredDataset { dataset }) => {
                    assert_eq!(dataset, &funding);
                }
                other => return Err(format!("expected MissingRequiredDataset, got {other:?}")),
            }
        }

        let um = id("um-kline")?;
        let gapped = Verified::from_parts(
            Table::Kline(Vec::new()),
            vec![gap("grid", 0, 10)],
            Vec::new(),
            Vec::new(),
        );
        let mut needs = BTreeMap::new();
        needs.insert(um.clone(), 5);
        let mut tables = BTreeMap::new();
        tables.insert(um, &gapped);
        let folds = [fold("gapped", 0, 0, 10, needs)];
        let stamps = preflight(&map, &tables, &folds);
        match &stamps[0].data_abort {
            Some(PreflightError::MissingRequiredDataset { dataset }) => {
                assert_eq!(dataset, &funding);
            }
            other => return Err(format!("gap outranked a missing dataset: {other:?}")),
        }
        Ok(())
    }

    #[test]
    fn gap_in_required_aborts_only_the_overlapping_fold() -> Result<(), String> {
        let mark = id("mark-price-kline")?;
        let start = 1_622_505_600_000_000;
        let minute = 60 * 1_000_000;
        let end = start + 29 * minute;
        let verified = Verified::from_parts(
            Table::Kline(Vec::new()),
            vec![gap("grid", start, end)],
            Vec::new(),
            Vec::new(),
        );
        let mut needs = BTreeMap::new();
        needs.insert(mark.clone(), 5);
        let (required, tables) = map_one(mark.clone(), Requirement::Required, &verified);
        let stamps = preflight(
            &required,
            &tables,
            &[
                fold(
                    "before",
                    start - minute,
                    start - minute,
                    start,
                    BTreeMap::new(),
                ),
                fold("inside", start, start, end, needs),
                fold("after", end, end, end + minute, BTreeMap::new()),
            ],
        );
        assert!(stamps[0].data_abort.is_none(), "{:?}", stamps[0].data_abort);
        match &stamps[1].data_abort {
            Some(PreflightError::GapInRequired {
                dataset,
                kind,
                start_us,
                end_us,
            }) => {
                assert_eq!(dataset, &mark);
                assert_eq!(kind, "grid");
                assert_eq!(*start_us, start);
                assert_eq!(*end_us, end);
            }
            other => return Err(format!("expected GapInRequired, got {other:?}")),
        }
        assert!(stamps[2].data_abort.is_none(), "{:?}", stamps[2].data_abort);

        let (required, tables) = map_one(mark.clone(), Requirement::Optional, &verified);
        let stamps = preflight(
            &required,
            &tables,
            &[fold("inside", start, start, end, BTreeMap::new())],
        );
        assert!(stamps[0].data_abort.is_none(), "{:?}", stamps[0].data_abort);
        assert_eq!(stamps[0].degraded.len(), 1);
        assert_eq!(stamps[0].degraded[0].kind, "grid");
        assert_eq!(stamps[0].degraded[0].dataset, mark);

        let funding = id("funding-rate")?;
        let june = day_us(2021, 6, 1)?;
        let july = day_us(2021, 7, 1)?;
        let month = Verified::from_parts(
            Table::Funding(Vec::new()),
            vec![gap("missing_file", june, july)],
            Vec::new(),
            Vec::new(),
        );
        let (required, tables) = map_one(funding.clone(), Requirement::Required, &month);
        let stamps = preflight(
            &required,
            &tables,
            &[
                fold("june", june, june, june + minute, BTreeMap::new()),
                fold("july", july, july, july + minute, BTreeMap::new()),
            ],
        );
        match &stamps[0].data_abort {
            Some(PreflightError::GapInRequired {
                dataset,
                kind,
                start_us,
                end_us,
            }) => {
                assert_eq!(dataset, &funding);
                assert_eq!(kind, "missing_file");
                assert_eq!(*start_us, june);
                assert_eq!(*end_us, july);
            }
            other => return Err(format!("expected month gap, got {other:?}")),
        }
        assert!(stamps[1].data_abort.is_none(), "{:?}", stamps[1].data_abort);

        let slot = june + 8 * 60 * minute;
        let pointed = Verified::from_parts(
            Table::Funding(Vec::new()),
            vec![gap("funding_slot", slot, slot)],
            Vec::new(),
            Vec::new(),
        );
        let (required, tables) = map_one(funding, Requirement::Required, &pointed);
        let stamps = preflight(
            &required,
            &tables,
            &[
                fold("touches", slot, slot, slot + 1, BTreeMap::new()),
                fold("ends", slot - minute, slot - minute, slot, BTreeMap::new()),
            ],
        );
        assert!(
            matches!(
                stamps[0].data_abort,
                Some(PreflightError::GapInRequired { .. })
            ),
            "{:?}",
            stamps[0].data_abort
        );
        assert!(stamps[1].data_abort.is_none(), "{:?}", stamps[1].data_abort);

        let both = Verified::from_parts(
            Table::Kline(Vec::new()),
            vec![
                gap("grid", end, end + minute),
                gap("grid", start, start + minute),
            ],
            Vec::new(),
            Vec::new(),
        );
        let (required, tables) = map_one(mark, Requirement::Required, &both);
        let stamps = preflight(
            &required,
            &tables,
            &[fold("wide", start, start, end + minute, BTreeMap::new())],
        );
        match &stamps[0].data_abort {
            Some(PreflightError::GapInRequired { start_us, .. }) => assert_eq!(*start_us, start),
            other => return Err(format!("expected the earliest gap, got {other:?}")),
        }

        let book = id("book-depth")?;
        let book_table = Verified::from_parts(
            Table::Book(Vec::new()),
            vec![gap("allowlisted_missing", start, end)],
            Vec::new(),
            Vec::new(),
        );
        let (required, tables) = map_one(book.clone(), Requirement::Required, &book_table);
        let stamps = preflight(
            &required,
            &tables,
            &[fold("book", start, start, end, BTreeMap::new())],
        );
        assert!(stamps[0].data_abort.is_none(), "{:?}", stamps[0].data_abort);
        assert_eq!(stamps[0].degraded.len(), 1);
        assert_eq!(stamps[0].degraded[0].kind, "fixed_bps");
        assert_eq!(stamps[0].degraded[0].dataset, book);
        Ok(())
    }

    #[test]
    fn warmup_ignores_excluded_rows_and_the_window_start() -> Result<(), String> {
        let rows = kline_rows()?;
        let um = id("um-kline")?;
        let warmup_start = avail(&rows[0]);
        let window_start = avail(&rows[2]);
        let window_end = window_start + 1;
        let plain = Verified::from_parts(
            Table::Kline(rows.clone()),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let mut needs = BTreeMap::new();
        needs.insert(um.clone(), 2);
        let (required, tables) = map_one(um.clone(), Requirement::Required, &plain);
        let stamps = preflight(
            &required,
            &tables,
            &[fold(
                "warm",
                warmup_start,
                window_start,
                window_end,
                needs.clone(),
            )],
        );
        assert!(stamps[0].data_abort.is_none(), "{:?}", stamps[0].data_abort);

        let window = ExcludedWindow {
            pk_start_us: pk(&rows[0]),
            pk_end_us: pk(&rows[1]),
            avail_start_us: avail(&rows[0]),
            avail_end_us: avail(&rows[0]) + 1,
            reason: "value_by_log".to_owned(),
        };
        let excluded = Verified::from_parts(
            Table::Kline(rows.clone()),
            Vec::new(),
            Vec::new(),
            vec![window],
        );
        assert!(excluded.is_excluded(pk(&rows[0])));
        assert!(!excluded.is_excluded(pk(&rows[1])));
        let (required, tables) = map_one(um.clone(), Requirement::Required, &excluded);
        let stamps = preflight(
            &required,
            &tables,
            &[fold(
                "excluded",
                warmup_start,
                window_start,
                window_end,
                needs,
            )],
        );
        match &stamps[0].data_abort {
            Some(PreflightError::InsufficientWarmup {
                dataset,
                have_rows,
                need_rows,
            }) => {
                assert_eq!(dataset, &um);
                assert_eq!(*have_rows, 1);
                assert_eq!(*need_rows, 2);
            }
            other => return Err(format!("expected InsufficientWarmup, got {other:?}")),
        }
        assert_eq!(stamps[0].excluded_windows.len(), 1);

        let mut boundary_needs = BTreeMap::new();
        boundary_needs.insert(um.clone(), 2);
        let (required, tables) = map_one(um.clone(), Requirement::Required, &plain);
        let stamps = preflight(
            &required,
            &tables,
            &[fold(
                "boundary",
                warmup_start,
                avail(&rows[1]),
                avail(&rows[1]) + 1,
                boundary_needs,
            )],
        );
        match &stamps[0].data_abort {
            Some(PreflightError::InsufficientWarmup {
                have_rows,
                need_rows,
                ..
            }) => {
                assert_eq!(*have_rows, 1);
                assert_eq!(*need_rows, 2);
            }
            other => return Err(format!("a row at window_start was counted: {other:?}")),
        }

        let book = id("book-depth")?;
        let empty =
            Verified::from_parts(Table::Book(Vec::new()), Vec::new(), Vec::new(), Vec::new());
        let mut book_needs = BTreeMap::new();
        book_needs.insert(book.clone(), 1);
        let (required, tables) = map_one(book.clone(), Requirement::Optional, &empty);
        let stamps = preflight(&required, &tables, &[fold("opt", 0, 1, 2, book_needs)]);
        match &stamps[0].data_abort {
            Some(PreflightError::InsufficientWarmup {
                dataset,
                have_rows: 0,
                need_rows: 1,
            }) => assert_eq!(dataset, &book),
            other => return Err(format!("optional warmup was skipped: {other:?}")),
        }

        let tables = BTreeMap::new();
        let mut absent_needs = BTreeMap::new();
        absent_needs.insert(book, 1);
        let (required, _) = map_one(id("book-depth")?, Requirement::Optional, &empty);
        let stamps = preflight(&required, &tables, &[fold("absent", 0, 1, 2, absent_needs)]);
        assert!(stamps[0].data_abort.is_none(), "{:?}", stamps[0].data_abort);

        let far = ExcludedWindow {
            pk_start_us: pk(&rows[0]),
            pk_end_us: pk(&rows[1]),
            avail_start_us: window_end + 10,
            avail_end_us: window_end + 11,
            reason: "value_by_log".to_owned(),
        };
        let outside = Verified::from_parts(Table::Kline(rows), Vec::new(), Vec::new(), vec![far]);
        let (required, tables) = map_one(um, Requirement::Required, &outside);
        let stamps = preflight(
            &required,
            &tables,
            &[fold(
                "far",
                warmup_start,
                window_start,
                window_end,
                BTreeMap::new(),
            )],
        );
        assert!(
            stamps[0].excluded_windows.is_empty(),
            "{:?}",
            stamps[0].excluded_windows
        );
        Ok(())
    }
}
