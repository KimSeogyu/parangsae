//! Leaky drives return a real output on the witness and fail the truncation check.
//! The failure is a different value, not `AfterDecision`.

#[path = "support/mod.rs"]
mod support;

use parangsae_core::BarInterval;
use parangsae_signals::{FundingZ, Tsmom, VolRegime, PROD_MAX_AGE_US};
use support::{
    check_bar_truncation, check_funding_truncation, drive_event_gate, funding_prod_witness,
    leaky_funding_horizon, leaky_next_bar_feature, leaky_next_bar_signal, tsmom_prod_witness,
    volregime_prod_witness, FUNDING_REGRESSION, H1_US, TSMOM_REGRESSION, VOLREGIME_REGRESSION,
};

fn assert_real_mismatch(err: &support::Mismatch) {
    let rendered = format!("{err:?}");
    assert_ne!(err.full, err.reference, "{rendered}");
    assert!(
        !rendered.contains("AfterDecision"),
        "leaky catch was only an admission error: {rendered}"
    );
}

mod leaky_fail_first {
    use super::*;

    #[test]
    fn tsmom_next_bar() {
        let signal = Tsmom::<24>::new(BarInterval::H1, 0.05).unwrap();
        let (bars, decision) = tsmom_prod_witness();
        let direct = leaky_next_bar_signal(&signal, &bars, decision, H1_US);
        assert!(direct.is_ok(), "{direct:?}");
        let err = check_bar_truncation(
            TSMOM_REGRESSION,
            BarInterval::H1,
            &bars,
            decision,
            |rows, d| leaky_next_bar_signal(&signal, rows, d, H1_US),
        )
        .expect_err("next-bar leak");
        assert_real_mismatch(&err);
    }

    #[test]
    fn volregime_next_bar() {
        let feature = VolRegime::<24, 168>::new(BarInterval::H1).unwrap();
        let (bars, decision) = volregime_prod_witness();
        let direct = leaky_next_bar_feature(&feature, &bars, decision, H1_US);
        assert!(direct.is_ok(), "{direct:?}");
        let err = check_bar_truncation(
            VOLREGIME_REGRESSION,
            BarInterval::H1,
            &bars,
            decision,
            |rows, d| leaky_next_bar_feature(&feature, rows, d, H1_US),
        )
        .expect_err("next-bar leak");
        assert_real_mismatch(&err);
    }

    #[test]
    fn funding_horizon() {
        let signal = FundingZ::<90>::new(BarInterval::H1, 2.0, PROD_MAX_AGE_US).unwrap();
        let (bars, funding, decision) = funding_prod_witness();
        let direct = leaky_funding_horizon(&signal, &bars, &funding, decision);
        assert!(direct.is_ok(), "{direct:?}");
        let err = check_funding_truncation(
            FUNDING_REGRESSION,
            &bars,
            &funding,
            decision,
            |bars, funding, d| leaky_funding_horizon(&signal, bars, funding, d),
        )
        .expect_err("funding horizon leak");
        assert_real_mismatch(&err);
    }

    #[test]
    fn funding_event_gate() {
        let signal = FundingZ::<90>::new(BarInterval::H1, 2.0, PROD_MAX_AGE_US).unwrap();
        let (bars, funding, decision) = funding_prod_witness();
        let direct = drive_event_gate(&signal, &bars, &funding, decision);
        assert!(direct.is_ok(), "{direct:?}");
        let err = check_funding_truncation(
            FUNDING_REGRESSION,
            &bars,
            &funding,
            decision,
            |bars, funding, d| drive_event_gate(&signal, bars, funding, d),
        )
        .expect_err("event gate leak");
        assert_real_mismatch(&err);
    }
}
