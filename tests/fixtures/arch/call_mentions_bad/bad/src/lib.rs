use exec::network_order_submit as y; // probe: use-alias

pub fn alias_path() {
    y::safe(); // probe: alias-call
}

use exec::gate as ymod;

pub fn through_mod() {
    ymod::network_order_submit(); // probe: mod-alias
}

pub fn typed_ptr() {
    let f: fn() = exec::network_order_submit; // probe: typed
    f(); // probe: typed-call
}

fn takes(_: fn()) {}

pub fn passed() {
    takes(exec::network_order_submit); // probe: argument
}

pub fn paren() {
    (exec::network_order_submit)(); // probe: paren
}

pub fn in_vec() {
    let _ = vec![exec::network_order_submit]; // probe: vec
}

macro_rules! call_it { // probe: macro_rules
    () => { exec::network_order_submit() };
}
