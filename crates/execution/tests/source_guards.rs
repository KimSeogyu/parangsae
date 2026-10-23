//! Fails if execution sources grow a float or an environment read.

use std::fs;
use std::path::Path;

const BANNED: &[&str] = &["f32", "f64", "std::env", "as f"];

#[test]
fn execution_sources_have_no_float_or_env() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs(&root, &mut files);
    assert!(!files.is_empty(), "execution src is missing");
    for path in files {
        let text = fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {path:?}: {err}"));
        for banned in BANNED {
            assert!(
                !text.contains(banned),
                "{} contains {banned}",
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
