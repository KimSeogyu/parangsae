//! Cucumber runner for `specs/features/instrument_rules.feature`.

#[path = "support/feature_floors.rs"]
mod feature_floors;
#[path = "support/feature_run.rs"]
mod feature_run;

use cucumber::{then, when};
use parangsae_core::instrument_spec::{rule_at, BracketPolicy, Field, RuleValue, UtcDateTime};

#[derive(Debug, Default, cucumber::World)]
struct RulesWorld {
    ticks: Option<i64>,
    message: Option<String>,
}

fn parse_utc(text: &str) -> Option<UtcDateTime> {
    let mut parts = text.split(['-', ' ', ':']);
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    let hour = parts.next()?.parse().ok()?;
    let minute = parts.next()?.parse().ok()?;
    let second = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    UtcDateTime::from_ymd_hms(year, month, day, hour, minute, second)
}

fn policy(name: &str) -> Option<BracketPolicy> {
    match name {
        "conservative" => Some(BracketPolicy::Conservative),
        "earliest" => Some(BracketPolicy::Earliest),
        "latest" => Some(BracketPolicy::Latest),
        "reject" => Some(BracketPolicy::Reject),
        _ => None,
    }
}

fn field(name: &str) -> Option<Field> {
    match name {
        "tick size" => Some(Field::TickSize),
        "taker fee" => Some(Field::FeeTakerVip0),
        _ => None,
    }
}

#[when(
    regex = r"^BTCUSDT (tick size|taker fee) is read at (\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}) UTC with policy (conservative|earliest|latest|reject)$"
)]
fn read_rule(world: &mut RulesWorld, field_name: String, at_text: String, policy_name: String) {
    world.ticks = None;
    world.message = None;
    let Some(at) = parse_utc(&at_text) else {
        world.message = Some(format!("unparsed time {at_text}"));
        return;
    };
    let Some(field) = field(&field_name) else {
        world.message = Some(format!("unknown field {field_name}"));
        return;
    };
    let Some(policy) = policy(&policy_name) else {
        world.message = Some(format!("unknown policy {policy_name}"));
        return;
    };
    match rule_at("BTCUSDT", field, at, policy) {
        Ok(rule) => match rule.value {
            RuleValue::Value(decimal) => world.ticks = Some(decimal.ticks()),
            RuleValue::Absent | RuleValue::Token(_) | RuleValue::EpochMillis(_) => {
                world.message = Some(format!("{field_name} is not a decimal"));
            }
        },
        Err(err) => world.message = Some(err.to_string()),
    }
}

#[then(regex = r"^the decimal ticks are (\d+)$")]
fn decimal_ticks(world: &mut RulesWorld, expected: i64) -> Result<(), String> {
    if world.message.is_some() {
        return Err(format!("lookup failed: {:?}", world.message));
    }
    if world.ticks != Some(expected) {
        return Err(format!("ticks {:?} != {expected}", world.ticks));
    }
    Ok(())
}

#[then("the lookup is unresolved")]
fn unresolved(world: &mut RulesWorld) -> Result<(), String> {
    let message = world.message.as_deref().unwrap_or("");
    if message.contains("unresolved") {
        Ok(())
    } else {
        Err(format!(
            "expected an unresolved lookup, got ticks {:?} message {:?}",
            world.ticks, world.message
        ))
    }
}

const FEATURE_GLOB: &str = "specs/features/instrument_rules.feature";

#[tokio::main]
async fn main() -> Result<(), String> {
    let files = feature_floors::expand_feature_glob(FEATURE_GLOB)?;
    let mut passed_steps = 0usize;
    for path in &files {
        let counts = feature_run::run_feature::<RulesWorld>(path).await?;
        passed_steps += counts.passed_steps;
    }
    feature_floors::enforce_floors("instrument_rules", &files, passed_steps)
}
