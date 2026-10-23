fn p<T>(_: T) {}

struct Clock;

impl Clock {
    fn as_secs_f64(&self) -> i32 {
        0
    }
}

pub fn probe() {
    let r = 0.5; // probe: unsuffixed
    let _ = r * 3.0 + 1e-3; // probe: exponent
    let _ = Clock.as_secs_f64() * 1.5; // probe: method
    p(0.5); // probe: generic
}
