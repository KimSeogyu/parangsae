use std::env::*; // probe: glob

pub fn glob_var() {
    let _ = var("X");
}

pub fn in_println() {
    println!("{}", std::env::var("X").unwrap_or_default()); // probe: println
}

pub fn in_vec() {
    let _ = vec![std::env::var("X")]; // probe: vec
}

macro_rules! read_env { // probe: macro_rules
    () => { std::env::var("X") };
}

pub fn in_macro_rules() {
    let _ = read_env!();
}

extern crate std as s2;

pub fn via_extern() {
    let _ = s2::env::var("X"); // probe: extern
}

pub fn via_options() {
    let _ = std::fs::File::options().write(true); // probe: options
}

pub fn via_symlink() {
    let _ = std::os::unix::fs::symlink("a", "b"); // probe: symlink
}

macro_rules! reads { // probe: unparsed-read
    () => { std::fs::read("a") };
}
