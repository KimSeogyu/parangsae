//! f64 Sharpe beside the integer result. The library crates do not use f64.

use parangsae_backtest::sharpe_ticks;

fn reference(levels: &[f64]) -> Option<(f64, f64)> {
    let returns: Vec<f64> = levels
        .windows(2)
        .map(|pair| (pair[1] - pair[0]) / pair[0])
        .collect();
    if returns.len() < 2 {
        return None;
    }
    let n = returns.len() as f64;
    let mean = returns.iter().sum::<f64>() / n;
    let variance = returns
        .iter()
        .map(|value| {
            let delta = value - mean;
            delta * delta
        })
        .sum::<f64>()
        / (n - 1.0);
    if variance == 0.0 {
        return None;
    }
    let daily = mean / variance.sqrt();
    Some((daily, daily * 365.0_f64.sqrt()))
}

#[test]
fn g9_integer_sharpe_is_within_one_tick_of_f64() {
    let levels = [100.0_f64, 110.0, 121.0, 121.0];
    let ticks: Vec<i64> = levels
        .iter()
        .map(|level| (level * 100_000_000.0) as i64)
        .collect();
    let (daily, annual) = sharpe_ticks(&ticks).unwrap();
    let (reference_daily, reference_annual) = reference(&levels).unwrap();
    let scale = 100_000_000.0_f64;
    let daily_ticks = daily.unwrap().ticks() as f64;
    let annual_ticks = annual.unwrap().ticks() as f64;
    assert!((reference_daily * scale - daily_ticks).abs() <= 1.0);
    assert!((reference_annual * scale - annual_ticks).abs() <= 1.0);
}
