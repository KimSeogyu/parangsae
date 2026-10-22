const TEXT: &str = "#[allow(clippy::unwrap_used)]";
const RAW: &str = r#"#![expect(clippy::panic)]"#;

fn kept() -> i64 {
    let _ = (TEXT, RAW);
    1
}
