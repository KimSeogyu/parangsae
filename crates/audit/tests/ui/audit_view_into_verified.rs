use parangsae_audit::AuditView;
use parangsae_collector::version::{Table, Verified};

fn convert(view: AuditView) -> Verified<Table> {
    view.into()
}

fn main() {
    let _ = convert;
}
