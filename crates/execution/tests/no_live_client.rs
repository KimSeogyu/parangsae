//! Fails if the execution crate grows a network client.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "filesystem walk helpers sit outside the #[test] item, so clippy.toml does not cover them"
)]

use std::fs;
use std::path::Path;

const BANNED: &[&str] = &[
    "std::net",
    "TcpStream",
    "UdpSocket",
    "reqwest",
    "tungstenite",
    "hyper",
    "wss://",
    "https://",
    "fapi.binance.com",
];

#[test]
fn execution_sources_have_no_order_transport() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    collect_rs(root.join("src").as_path(), &mut files);
    assert!(!files.is_empty(), "execution src is missing");
    for path in files {
        let text = fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {path:?}: {err}"));
        for banned in BANNED {
            assert!(
                !text.contains(banned),
                "{} contains {banned}, which this scaffold does not ship",
                path.display()
            );
        }
    }
}

fn collect_rs(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|err| panic!("read {}: {err}", dir.display())) {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}
