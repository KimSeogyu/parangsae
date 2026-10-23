//! External crates cannot construct `Causal`. The public observation API
//! is `Observation::new` and `DecisionTime::from_micros`.

#[test]
fn external_code_cannot_construct_causal() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/observation_new.rs");
    cases.pass("tests/ui/observation_value.rs");
    cases.pass("tests/ui/observation_with_value.rs");
    cases.pass("tests/ui/decision_time_from_micros.rs");
    cases.pass("tests/ui/observation_eq.rs");
    cases.pass("tests/ui/observation_debug.rs");
    cases.compile_fail("tests/ui/causal_private.rs");
}
