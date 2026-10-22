//! T6: `RunId` and `Receipt` have no conversion into a time.
//!
//! Each compile-fail program is a complete `fn f(run_id, r) -> UtcMicros` that
//! fails only because the named conversion or accessor is absent. The matching
//! `*_ok` program has the same shape and uses only allowed calls.

#[test]
fn receipt_conversions_fail_to_compile() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/run_id_into.rs");
    tests.compile_fail("tests/ui/utc_from_run_id.rs");
    tests.compile_fail("tests/ui/available_from_run_id.rs");
    tests.compile_fail("tests/ui/archive_run_micros.rs");
    tests.compile_fail("tests/ui/duration_from_run_id.rs");
    tests.pass("tests/ui/run_id_into_ok.rs");
    tests.pass("tests/ui/utc_from_run_id_ok.rs");
    tests.pass("tests/ui/available_from_run_id_ok.rs");
    tests.pass("tests/ui/archive_run_micros_ok.rs");
    tests.pass("tests/ui/duration_from_run_id_ok.rs");
}
