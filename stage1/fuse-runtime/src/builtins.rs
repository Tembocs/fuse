//! Built-in functions available to all Fuse programs.

use crate::value::FuseValue;

pub fn fuse_println(val: &FuseValue) {
    println!("{val}");
}

pub fn fuse_eprintln(val: &FuseValue) {
    eprintln!("{val}");
}
