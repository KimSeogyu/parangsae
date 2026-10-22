#[test]
fn registry_names_are_private_and_classify_is_public() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/classify_with.rs");
    tests.compile_fail("tests/ui/rule_registry.rs");
    tests.compile_fail("tests/ui/metric_missing_rules.rs");
    tests.compile_fail("tests/ui/row_id.rs");
    tests.pass("tests/ui/classify_current.rs");
    tests.pass("tests/ui/rule_version_match.rs");
}
