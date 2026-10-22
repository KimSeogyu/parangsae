//! Fails if the execution crate grows a network client.

use std::fs;
use std::path::{Path, PathBuf};

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
fn execution_sources_have_no_order_transport() -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    collect_rs(root.join("src").as_path(), &mut files)?;
    if files.is_empty() {
        return Err("execution src is missing".to_owned());
    }
    for path in files {
        let text = fs::read_to_string(&path).map_err(|err| format!("read {path:?}: {err}"))?;
        for banned in BANNED {
            if text.contains(banned) {
                return Err(format!(
                    "{} contains {banned}, which this scaffold does not ship",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|err| format!("read {}: {err}", dir.display()))? {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    Ok(())
}
