pub fn scale() -> f64 {
    let r = 0.5;
    let _ = r * 3.0 + 1e-3;
    let _ = as_secs_f64() * 1.5;
    p(0.5);
    1.0
}

fn as_secs_f64() -> f64 {
    1.0
}

fn p(_: f64) {}
