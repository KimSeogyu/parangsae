//! External crates cannot bypass the gate or compare the private payload.

#[test]
fn external_code_cannot_bypass_the_causal_gate() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/observation_new.rs");
    cases.compile_fail("tests/ui/observation_value.rs");
    cases.compile_fail("tests/ui/observation_with_value.rs");
    cases.compile_fail("tests/ui/decision_time_from_micros.rs");
    cases.compile_fail("tests/ui/observation_for_test.rs");
    cases.compile_fail("tests/ui/decision_time_for_test.rs");
    cases.compile_fail("tests/ui/observation_eq.rs");
    cases.pass("tests/ui/observation_debug.rs");
}
