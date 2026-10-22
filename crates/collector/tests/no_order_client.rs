//! The collector may download public Vision bytes. It must not grow an order client.

use std::fs;
use std::path::{Path, PathBuf};

const BANNED: &[&str] = &["BINANCE_API_KEY", "/fapi/v1/order", "api.binance.com"];

#[test]
fn collector_sources_do_not_name_an_order_endpoint() -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs(&root, &mut files)?;
    if files.is_empty() {
        return Err("collector src is missing".to_owned());
    }
    let mut saw_vision_host = false;
    for path in &files {
        let text = fs::read_to_string(path).map_err(|err| format!("read {path:?}: {err}"))?;
        if text.contains("data.binance.vision") {
            saw_vision_host = true;
        }
        for banned in BANNED {
            if text.contains(banned) {
                return Err(format!("{} contains {banned}", path.display()));
            }
        }
        if text.contains("reqwest") && !path.ends_with("fetch.rs") {
            return Err(format!(
                "{} uses reqwest; only the Vision downloader may",
                path.display()
            ));
        }
    }
    if !saw_vision_host {
        return Err("the Vision download host should be named".to_owned());
    }
    Ok(())
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|err| format!("read {}: {err}", dir.display()))? {
        let path = entry.map_err(|err| err.to_string())?.path();
        if path.is_dir() {
            collect_rs(&path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
}
