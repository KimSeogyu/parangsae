#![cfg(unix)]

use std::ffi::OsString;
use std::fs::{self, File};
use std::os::fd::OwnedFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Command, Stdio};

fn command(root: &Path, inputs: &Path) -> Command {
    let out = root.join("new-output");
    let mut child = Command::new(env!("CARGO_BIN_EXE_c1_batch"));
    child
        .arg("--inputs")
        .arg(inputs)
        .arg("--data-root")
        .arg(root.join("snapshot"))
        .arg("--out")
        .arg(&out)
        .arg("--ledger")
        .arg(out.join("trials.jsonl"))
        .arg("--attempt")
        .arg("4")
        .stdin(Stdio::null());
    child
}

fn assert_no_start(root: &Path) {
    assert!(!root.join("new-output").exists());
    assert!(!root.join("new-output/trials.jsonl").exists());
}

#[test]
fn actual_cli_entry_failure_hands_off_exact_request_json_including_unix_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    for name in [
        OsString::from("missing.json"),
        OsString::from_vec(b"missing-\xff.json".to_vec()),
    ] {
        let inputs = root.join(name);
        let output = command(root, &inputs).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(value["schema"], "parangsae-c1-failure-handoff-v1");
        assert!(value["entry_error"]
            .as_str()
            .is_some_and(|text| !text.is_empty()));
        assert!(value["batch"].is_null());
        assert!(value["report_status"].is_null());
        let request = &value["request"];
        match inputs.to_str() {
            Some(text) => assert_eq!(request["inputs"], text),
            None => assert_eq!(
                request["inputs"],
                serde_json::json!({
                    "encoding":"unix_bytes", "bytes":inputs.as_os_str().as_bytes()
                })
            ),
        }
        assert_eq!(
            request["data_root"],
            root.join("snapshot").to_str().unwrap()
        );
        assert_eq!(request["out"], root.join("new-output").to_str().unwrap());
        assert_eq!(
            request["ledger"],
            root.join("new-output/trials.jsonl").to_str().unwrap()
        );
        assert_eq!(request["attempt"], 4);
        assert_no_start(root);
    }
}

#[test]
fn actual_cli_disconnected_stderr_returns_unacknowledged_exit_without_starting() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let (writer, receiver) = UnixStream::pair().unwrap();
    // Close the peer before spawning: this valid descriptor fails with EPIPE.
    drop(receiver);
    let writer: OwnedFd = writer.into();
    let output = command(root, &root.join("missing.json"))
        .stdout(Stdio::piped())
        .stderr(Stdio::from(writer))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(74));
    assert!(output.stdout.is_empty());
    assert_no_start(root);
}

#[test]
fn actual_cli_read_only_stderr_returns_unacknowledged_exit_without_changing_file() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let sink = root.join("read-only-stderr");
    let original = b"retained stderr sentinel\n";
    fs::write(&sink, original).unwrap();
    let readonly = File::open(&sink).unwrap();
    let output = command(root, &root.join("missing.json"))
        .stdout(Stdio::piped())
        .stderr(Stdio::from(readonly))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(74));
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read(&sink).unwrap(), original);
    assert_no_start(root);
}
