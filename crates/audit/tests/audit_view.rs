#[test]
fn audit_view_is_not_a_verified_table() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/audit_view_into_verified.rs");
    tests.compile_fail("tests/ui/rule_version_string.rs");
    tests.pass("tests/ui/rule_version_match.rs");
}
