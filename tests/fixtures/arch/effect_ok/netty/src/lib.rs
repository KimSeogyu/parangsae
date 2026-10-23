use std::net::*;

pub fn listener() -> TcpListener {
    loop {}
}

pub fn read_only() {
    let _ = std::fs::read("a");
    let _ = std::fs::File::open("a");
}
