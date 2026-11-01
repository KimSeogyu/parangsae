//! Read-only catalogue discovery and a synthetic execution-plan demonstration.
use parangsae_backtest::catalog::{evaluate, Family, InputUniverse, Output, Request};
fn main() {
    match std::env::args().nth(1).as_deref() {
        None | Some("list") => {
            for family in Family::ALL {
                println!(
                    "{:02} {:26} {:?} BTC-USD-M research={} (all: supplied-offline)",
                    family.source_number(),
                    family.name(),
                    family.role(),
                    family.supports(InputUniverse::BtcUsdM)
                );
            }
        }
        Some("twap") => {
            let result = evaluate(
                InputUniverse::BtcUsdM,
                Request::Twap {
                    lots: 10,
                    start_micros: 1,
                    end_micros: 10,
                    count: 3,
                },
            )
            .expect("synthetic schedule");
            let Output::Schedule(plan) = result else {
                unreachable!("typed schedule route")
            };
            println!("Synthetic proposal only; not orders: {:?}", plan.slices());
        }
        Some(_) => {
            eprintln!("usage: catalog18 [list|twap]");
            std::process::exit(2);
        }
    }
}
