//! The collector may download public Vision bytes. It must not grow an order client.

use std::fs;
use std::path::Path;

const BANNED: &[&str] = &["BINANCE_API_KEY", "/fapi/v1/order", "api.binance.com"];

#[test]
fn collector_sources_do_not_name_an_order_endpoint() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs(&root, &mut files);
    assert!(!files.is_empty());
    let mut saw_vision_host = false;
    for path in &files {
        let text = fs::read_to_string(path).unwrap_or_else(|err| panic!("read {path:?}: {err}"));
        if text.contains("data.binance.vision") {
            saw_vision_host = true;
        }
        for banned in BANNED {
            assert!(
                !text.contains(banned),
                "{} contains {banned}",
                path.display()
            );
        }
        if text.contains("reqwest") {
            assert!(
                path.ends_with("fetch.rs"),
                "{} uses reqwest; only the Vision downloader may",
                path.display()
            );
        }
    }
    assert!(saw_vision_host, "the Vision download host should be named");
}

fn collect_rs(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|err| panic!("read {}: {err}", dir.display())) {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}
