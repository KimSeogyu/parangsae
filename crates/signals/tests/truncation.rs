//! Truncation invariance. These cases sample the gate. They are not a proof.
//!
//! The H4 volregime cap is 223 bars, past this generator's 200-bar ceiling.
//! That emit is pinned by `warmup_emit`.

#[path = "support/mod.rs"]
mod support;

use parangsae_core::BarInterval;
use parangsae_signals::{
    drive, drive_feature, drive_funding_z, FundingZ, Tsmom, VolRegime, PROD_MAX_AGE_US, SIGNAL_IDS,
};
use std::collections::BTreeSet;
use support::{
    check_bar_truncation, check_funding_truncation, funding_prod_witness, tsmom_prod_witness,
    volregime_prod_witness, FUNDING_REGRESSION, TSMOM_REGRESSION, VOLREGIME_REGRESSION,
};

mod truncation {
    use super::*;

    #[test]
    fn tsmom() {
        let signal = Tsmom::<24>::new(BarInterval::H1, 0.05).unwrap();
        let (bars, decision) = tsmom_prod_witness();
        check_bar_truncation(
            TSMOM_REGRESSION,
            BarInterval::H1,
            &bars,
            decision,
            |rows, d| drive(&signal, rows, d),
        )
        .unwrap_or_else(|err| panic!("{err:?}"));
    }

    #[test]
    fn volregime() {
        let feature = VolRegime::<24, 168>::new(BarInterval::H1).unwrap();
        let (bars, decision) = volregime_prod_witness();
        check_bar_truncation(
            VOLREGIME_REGRESSION,
            BarInterval::H1,
            &bars,
            decision,
            |rows, d| drive_feature(&feature, rows, d),
        )
        .unwrap_or_else(|err| panic!("{err:?}"));
    }

    #[test]
    fn funding_z() {
        let signal = FundingZ::<90>::new(BarInterval::H1, 2.0, PROD_MAX_AGE_US).unwrap();
        let (bars, funding, decision) = funding_prod_witness();
        check_funding_truncation(
            FUNDING_REGRESSION,
            &bars,
            &funding,
            decision,
            |bars, funding, d| drive_funding_z(&signal, bars, funding, d),
        )
        .unwrap_or_else(|err| panic!("{err:?}"));
    }

    #[test]
    fn ids_covered() {
        let tested: BTreeSet<&str> = ["tsmom", "volregime", "funding_z"].into_iter().collect();
        let shipped: BTreeSet<&str> = SIGNAL_IDS.iter().copied().collect();
        assert_eq!(tested, shipped);
    }
}
