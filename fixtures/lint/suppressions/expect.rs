#[expect(clippy::unwrap_used)]
fn kept(value: Option<i64>) -> i64 {
    value.unwrap()
}
