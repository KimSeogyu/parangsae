pub struct Ledger {
    cash: i64,
}

impl Ledger {
    pub fn new() -> Self {
        Self { cash: 0 }
    }

    pub fn set(&mut self, value: i64) {
        self.cash = value;
    }
}
