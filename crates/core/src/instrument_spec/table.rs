//! BTCUSDT USD-M rules. The seed is `fixtures/instrument/btcusdt_um_rules.csv`.
//! A test checks this array against that file. Corrections append a revision.

use super::clock::{UtcDate, UtcDateTime};
use super::types::{
    Boundary, Confidence, CoverageGap, Field, InstrumentRule, RuleValue, UnresolvedItem,
};
use crate::time::UtcMicros;

const fn at(micros: i64) -> UtcDateTime {
    UtcDateTime::from_micros(UtcMicros::from_micros(micros))
}

const fn day(year: i16, month: u8, day: u8) -> UtcDate {
    UtcDate::known(year, month, day)
}

/// 2019-09-25 08:00:00 UTC. `onboardDate` 1569398400000 ms.
pub const LISTING_US: i64 = 1_569_398_400_000_000;
/// 2020-01-01 00:00:00 UTC. Backtests start here.
pub const BACKTEST_START_US: i64 = 1_577_836_800_000_000;
/// exchangeInfo `serverTime` 1609789862039 (2021-01-04 19:51:02.039 UTC).
pub const WB2021_US: i64 = 1_609_789_862_039_000;
/// exchangeInfo `serverTime` 1647604838598 (2022-03-18 12:00:38.598 UTC).
pub const WB2022_US: i64 = 1_647_604_838_598_000;
/// 2022-02-15 03:30:00 UTC. Tick 0.01 → 0.10.
pub const TICK_SWITCH_US: i64 = 1_644_895_800_000_000;
/// Wayback FAQ capture 2020-09-30 10:35:54 UTC.
pub const FEE_FAQ_2020_US: i64 = 1_601_462_154_000_000;
/// Wayback FAQ capture 2021-01-28 06:41:21 UTC.
pub const FEE_FAQ_2021_US: i64 = 1_611_816_081_000_000;
/// Wayback FAQ capture 2023-06-21 19:20:25 UTC. Taker fee still 0.0004.
pub const FEE_FAQ_2023_US: i64 = 1_687_375_225_000_000;
/// Wayback FAQ capture 2024-02-20 06:41:15 UTC. Taker fee is 0.0005.
pub const FEE_FAQ_2024_US: i64 = 1_708_411_275_000_000;
pub const NOTIONAL_1_US: i64 = 1_611_558_000_000_000;
pub const NOTIONAL_5_US: i64 = 1_614_135_600_000_000;
pub const NOTIONAL_100_US: i64 = 1_698_919_200_000_000;
pub const NOTIONAL_50_US: i64 = 1_776_148_200_000_000;

pub const LISTING: UtcDateTime = at(LISTING_US);
pub const BACKTEST_START: UtcDateTime = at(BACKTEST_START_US);
pub const WB2021: UtcDateTime = at(WB2021_US);
pub const WB2022: UtcDateTime = at(WB2022_US);
pub const TICK_SWITCH: UtcDateTime = at(TICK_SWITCH_US);
pub const FEE_FAQ_2020: UtcDateTime = at(FEE_FAQ_2020_US);
pub const FEE_FAQ_2021: UtcDateTime = at(FEE_FAQ_2021_US);
pub const FEE_FAQ_2023: UtcDateTime = at(FEE_FAQ_2023_US);
pub const FEE_FAQ_2024: UtcDateTime = at(FEE_FAQ_2024_US);
pub const NOTIONAL_1_AT: UtcDateTime = at(NOTIONAL_1_US);
pub const NOTIONAL_5_AT: UtcDateTime = at(NOTIONAL_5_US);
pub const NOTIONAL_100_AT: UtcDateTime = at(NOTIONAL_100_US);
pub const NOTIONAL_50_AT: UtcDateTime = at(NOTIONAL_50_US);

/// Open on the left, closed on the right: the change is in `(earliest, latest]`,
/// and the two observation instants themselves are certain.
pub const MARKET_MAX_CHANGE: Boundary = Boundary::Bracketed {
    earliest: WB2021,
    latest: WB2022,
};
pub const TAKER_FEE_CHANGE: Boundary = Boundary::Bracketed {
    earliest: FEE_FAQ_2023,
    latest: FEE_FAQ_2024,
};
/// VIP0 fee value known at the 2021-01-28 FAQ, not before listing.
pub const VIP0_FEE_START: Boundary = Boundary::Bracketed {
    earliest: LISTING,
    latest: FEE_FAQ_2021,
};
pub const BNB_DISCOUNT_START: Boundary = Boundary::Bracketed {
    earliest: LISTING,
    latest: FEE_FAQ_2020,
};

pub const TICK_0_01: i64 = 1_000_000;
pub const TICK_0_10: i64 = 10_000_000;
pub const STEP_0_001: i64 = 100_000;
pub const QTY_1000: i64 = 100_000_000_000;
pub const QTY_120: i64 = 12_000_000_000;
pub const NOTIONAL_1: i64 = 100_000_000;
pub const NOTIONAL_5: i64 = 500_000_000;
pub const NOTIONAL_100: i64 = 10_000_000_000;
pub const NOTIONAL_50: i64 = 5_000_000_000;
pub const FEE_MAKER: i64 = 20_000;
pub const FEE_TAKER_4: i64 = 40_000;
pub const FEE_TAKER_5: i64 = 50_000;
pub const FEE_BNB: i64 = 10_000_000;
pub const HOURS_8: i64 = 800_000_000;
pub const PRECISION_2: i64 = 200_000_000;
pub const PRECISION_3: i64 = 300_000_000;
pub const ONBOARD_MS: i64 = 1_569_398_400_000;

/// Two UTC days after a tick coarsening. Vision cited off-0.1 prints on
/// 2022-02-15 after 03:30 and on 2022-02-16, and none later.
pub const TICK_COARSEN_GRACE_DAYS: i64 = 2;
pub const TICK_COARSEN_GRACE_US: i64 = 172_800_000_000;

const RETRIEVED: UtcDate = day(2026, 10, 22);

const WB2021_URL: &str =
    "https://web.archive.org/web/20210104195101id_/https://fapi.binance.com/fapi/v1/exchangeInfo";
const WB2022_URL: &str =
    "https://web.archive.org/web/20220318130953id_/https://fapi.binance.com/fapi/v1/exchangeInfo";
const ANN_MN1: &str = "https://www.binance.com/en/support/announcement/new-minimum-order-notional-limit-rule-for-usd%E2%93%A2-m-futures-76719bbaeeb847bbac4daa2906fcdcc0";
const ANN_MN5: &str = "https://www.binance.com/en/support/announcement/updates-to-usd%E2%93%A2-m-futures-trading-rules-1d762daf60dd417f9d8a05d44b06a25c";
const ANN_MN100: &str =
    "https://www.binance.com/en/support/announcement/detail/e4384cba297a4bd2a154be644d5d76f9";
const ANN_MN50: &str = "https://www.binance.com/en-IN/square/post/310575545686066";
const FAQ_2020: &str = "https://web.archive.org/web/20200930103554/https://www.binance.com/en/support/faq/360033544231";
const FAQ_2021: &str = "https://web.archive.org/web/20210128064121/https://www.binance.com/en/support/faq/360033544231";
const FAQ_2023: &str = "https://web.archive.org/web/20230621192025/https://www.binance.com/en/support/faq/binance-futures-fee-structure-fee-calculations-360033544231";
const FAQ_2024: &str = "https://web.archive.org/web/20240220064115/https://www.binance.com/en/support/faq/binance-futures-fee-structure-fee-calculations-360033544231";
const VIS_FR: &str = "https://data.binance.vision/data/futures/um/monthly/fundingRate/BTCUSDT/";

const fn row(
    field: Field,
    value: RuleValue,
    valid_from: Boundary,
    valid_to: Option<Boundary>,
    source: &'static str,
    source_date: UtcDate,
    confidence: Confidence,
) -> InstrumentRule {
    InstrumentRule {
        symbol: "BTCUSDT",
        venue: "binance-usdm",
        field,
        value,
        valid_from,
        valid_to,
        source,
        source_date,
        retrieved: RETRIEVED,
        confidence,
        revision: 1,
    }
}

/// Append-only BTCUSDT USD-M rows, CSV order. Revision 1.
pub static BTCUSDT_RULES: &[InstrumentRule] = &[
    row(
        Field::ContractType,
        RuleValue::Token("PERPETUAL"),
        Boundary::Exact(LISTING),
        None,
        WB2021_URL,
        day(2021, 1, 4),
        Confidence::VerifiedArchive,
    ),
    row(
        Field::OnboardDateMs,
        RuleValue::EpochMillis(ONBOARD_MS),
        Boundary::Exact(LISTING),
        None,
        WB2022_URL,
        day(2022, 3, 18),
        Confidence::VerifiedArchive,
    ),
    row(
        Field::TickSize,
        RuleValue::decimal(TICK_0_01),
        Boundary::Exact(LISTING),
        Some(Boundary::Exact(TICK_SWITCH)),
        WB2021_URL,
        day(2021, 1, 4),
        Confidence::VerifiedArchive,
    ),
    row(
        Field::TickSize,
        RuleValue::decimal(TICK_0_10),
        Boundary::Exact(TICK_SWITCH),
        None,
        WB2022_URL,
        day(2022, 3, 18),
        Confidence::VerifiedArchive,
    ),
    row(
        Field::StepSize,
        RuleValue::decimal(STEP_0_001),
        Boundary::Exact(LISTING),
        None,
        WB2022_URL,
        day(2022, 3, 18),
        Confidence::VerifiedArchive,
    ),
    row(
        Field::MinQty,
        RuleValue::decimal(STEP_0_001),
        Boundary::Exact(LISTING),
        None,
        WB2022_URL,
        day(2022, 3, 18),
        Confidence::VerifiedArchive,
    ),
    row(
        Field::LotMaxQty,
        RuleValue::decimal(QTY_1000),
        Boundary::Exact(WB2021),
        None,
        WB2022_URL,
        day(2022, 3, 18),
        Confidence::VerifiedArchive,
    ),
    row(
        Field::MarketMaxQty,
        RuleValue::decimal(QTY_1000),
        Boundary::Exact(WB2021),
        Some(MARKET_MAX_CHANGE),
        WB2021_URL,
        day(2021, 1, 4),
        Confidence::VerifiedArchive,
    ),
    row(
        Field::MarketMaxQty,
        RuleValue::decimal(QTY_120),
        MARKET_MAX_CHANGE,
        None,
        WB2022_URL,
        day(2022, 3, 18),
        Confidence::VerifiedArchive,
    ),
    row(
        Field::MinNotional,
        RuleValue::Absent,
        Boundary::Exact(LISTING),
        Some(Boundary::Exact(NOTIONAL_1_AT)),
        ANN_MN1,
        day(2021, 1, 21),
        Confidence::VerifiedDoc,
    ),
    row(
        Field::MinNotional,
        RuleValue::decimal(NOTIONAL_1),
        Boundary::Exact(NOTIONAL_1_AT),
        Some(Boundary::Exact(NOTIONAL_5_AT)),
        ANN_MN1,
        day(2021, 1, 21),
        Confidence::VerifiedDoc,
    ),
    row(
        Field::MinNotional,
        RuleValue::decimal(NOTIONAL_5),
        Boundary::Exact(NOTIONAL_5_AT),
        Some(Boundary::Exact(NOTIONAL_100_AT)),
        ANN_MN5,
        day(2021, 2, 23),
        Confidence::VerifiedArchive,
    ),
    row(
        Field::MinNotional,
        RuleValue::decimal(NOTIONAL_100),
        Boundary::Exact(NOTIONAL_100_AT),
        Some(Boundary::Exact(NOTIONAL_50_AT)),
        ANN_MN100,
        day(2023, 10, 27),
        Confidence::VerifiedDoc,
    ),
    row(
        Field::MinNotional,
        RuleValue::decimal(NOTIONAL_50),
        Boundary::Exact(NOTIONAL_50_AT),
        None,
        ANN_MN50,
        day(2026, 4, 9),
        Confidence::VerifiedDoc,
    ),
    row(
        Field::PricePrecision,
        RuleValue::decimal(PRECISION_2),
        Boundary::Exact(WB2021),
        None,
        WB2022_URL,
        day(2022, 3, 18),
        Confidence::VerifiedArchive,
    ),
    row(
        Field::QuantityPrecision,
        RuleValue::decimal(PRECISION_3),
        Boundary::Exact(WB2021),
        None,
        WB2022_URL,
        day(2022, 3, 18),
        Confidence::VerifiedArchive,
    ),
    row(
        Field::FundingIntervalHours,
        RuleValue::decimal(HOURS_8),
        Boundary::Exact(LISTING),
        None,
        VIS_FR,
        day(2026, 9, 1),
        Confidence::CorroboratedFromData,
    ),
    row(
        Field::FeeMakerVip0,
        RuleValue::decimal(FEE_MAKER),
        VIP0_FEE_START,
        None,
        FAQ_2021,
        day(2021, 1, 28),
        Confidence::VerifiedArchive,
    ),
    row(
        Field::FeeTakerVip0,
        RuleValue::decimal(FEE_TAKER_4),
        VIP0_FEE_START,
        Some(TAKER_FEE_CHANGE),
        FAQ_2023,
        day(2023, 6, 21),
        Confidence::VerifiedArchive,
    ),
    row(
        Field::FeeTakerVip0,
        RuleValue::decimal(FEE_TAKER_5),
        TAKER_FEE_CHANGE,
        None,
        FAQ_2024,
        day(2024, 2, 20),
        Confidence::VerifiedArchive,
    ),
    row(
        Field::FeeBnbDiscount,
        RuleValue::decimal(FEE_BNB),
        BNB_DISCOUNT_START,
        None,
        FAQ_2020,
        day(2020, 9, 30),
        Confidence::VerifiedArchive,
    ),
];

/// Certain coverage starts after 2020-01-01. A new gap that is not listed
/// fails [`super::audit_continuity`].
pub static KNOWN_COVERAGE_GAPS: &[CoverageGap] = &[
    CoverageGap {
        field: Field::LotMaxQty,
        known_from: WB2021,
        reason: "first exchangeInfo snapshot; the seed does not give an earlier lot max",
    },
    CoverageGap {
        field: Field::MarketMaxQty,
        known_from: WB2021,
        reason: "first exchangeInfo snapshot; market max qty before 2021-01-04 is unknown",
    },
    CoverageGap {
        field: Field::PricePrecision,
        known_from: WB2021,
        reason: "display hint from the first snapshot; it is not a tick size",
    },
    CoverageGap {
        field: Field::QuantityPrecision,
        known_from: WB2021,
        reason: "first exchangeInfo snapshot; earlier quantity precision is unknown",
    },
    CoverageGap {
        field: Field::FeeMakerVip0,
        known_from: FEE_FAQ_2021,
        reason: "VIP0 maker before the 2021-01-28 FAQ is unresolved (2019-09..2021-01)",
    },
    CoverageGap {
        field: Field::FeeTakerVip0,
        known_from: FEE_FAQ_2021,
        reason: "VIP0 taker before the 2021-01-28 FAQ is unresolved (2019-09..2021-01)",
    },
    CoverageGap {
        field: Field::FeeBnbDiscount,
        known_from: FEE_FAQ_2020,
        reason: "BNB discount is confirmed at the 2020-09-30 FAQ; earlier is unknown",
    },
];

/// Items in section (d) of the instrument-constants note. Not guessed values.
pub static UNRESOLVED_ITEMS: &[UnresolvedItem] = &[
    UnresolvedItem {
        id: "exchange_info_2026_10",
        summary: "current exchangeInfo (maxPrice, PERCENT_PRICE, new filters, live pricePrecision) needs a non-restricted host",
    },
    UnresolvedItem {
        id: "market_lot_max_qty_date",
        summary: "exact instant MARKET_LOT_SIZE.maxQty changed from 1000 to 120, between the 2021-01-04 and 2022-03-18 snapshots",
    },
    UnresolvedItem {
        id: "vip0_taker_fee_date",
        summary: "exact instant VIP0 taker fee changed from 0.0004 to 0.0005, between the 2023-06-21 and 2024-02-20 FAQ captures",
    },
    UnresolvedItem {
        id: "vip0_fees_before_2021_01_28",
        summary: "VIP0 maker and taker fees from listing (2019-09) through the 2021-01-28 FAQ are not in a text table",
    },
    UnresolvedItem {
        id: "tick_size_announcement_url",
        summary: "official binance.com URL for the 2022-02-15 tick-size announcement was not retrieved",
    },
    UnresolvedItem {
        id: "min_notional_deadline_instants",
        summary: "min notional 100 was announced \"by\" 2023-11-02 10:00 and the 2026-04-14 50 USDT change was a rollout, not a proven instant",
    },
    UnresolvedItem {
        id: "funding_interval_funding_info",
        summary: "per-symbol funding interval from /fapi/v1/fundingInfo needs a non-restricted host; Vision samples are 8h",
    },
];

/// No unverified fill row is cleared for paper. The list is empty on purpose.
pub static PAPER_BLOCKED: &[super::types::PaperBlock] = &[];
