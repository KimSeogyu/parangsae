//! Deliberate lint violations. `cargo clippy --all-targets -- -D warnings` must fail.
#![deny(clippy::arithmetic_side_effects)]

pub enum Side {
    Bid,
    Ask,
    Mid,
}

pub fn unwraps(value: Option<i64>) -> i64 {
    value.unwrap()
}

pub fn expects(value: Option<i64>) -> i64 {
    value.expect("present")
}

pub fn panics() -> i64 {
    panic!("no")
}

pub fn unfinished() -> i64 {
    todo!()
}

pub fn missing() -> i64 {
    unimplemented!()
}

pub fn debugs(value: i64) -> i64 {
    dbg!(value)
}

pub fn wild(side: Side) -> i64 {
    match side {
        Side::Bid => 1,
        _ => 0,
    }
}

pub fn truncates(value: u64) -> u8 {
    value as u8
}

pub fn wraps(value: u64) -> i64 {
    value as i64
}

pub fn loses_sign(value: i64) -> u64 {
    value as u64
}

pub fn adds_float(value: f64) -> f64 {
    value + 1.0
}

pub fn increments(value: i64) -> i64 {
    value + 1
}

pub fn prints_out(value: i64) -> i64 {
    println!("{value}");
    value
}

pub fn prints_err(value: i64) -> i64 {
    eprintln!("{value}");
    value
}

pub fn leaves() -> i64 {
    std::process::exit(1)
}

#[allow(clippy::unwrap_used)]
pub fn allowed_without_reason(value: Option<i64>) -> i64 {
    value.unwrap()
}

pub fn exhibit() -> usize {
    let _ = (
        unwraps as fn(Option<i64>) -> i64,
        expects as fn(Option<i64>) -> i64,
        panics as fn() -> i64,
        unfinished as fn() -> i64,
        missing as fn() -> i64,
        debugs as fn(i64) -> i64,
        wild as fn(Side) -> i64,
        truncates as fn(u64) -> u8,
        wraps as fn(u64) -> i64,
        loses_sign as fn(i64) -> u64,
        adds_float as fn(f64) -> f64,
        increments as fn(i64) -> i64,
        prints_out as fn(i64) -> i64,
        prints_err as fn(i64) -> i64,
        leaves as fn() -> i64,
        allowed_without_reason as fn(Option<i64>) -> i64,
    );
    0
}
