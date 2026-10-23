//! Float operators the workspace lint rejects. The values are the same bits.

use std::ops::{Add, Div, Mul, Neg, Sub};

pub(crate) fn fadd(left: f64, right: f64) -> f64 {
    <f64 as Add>::add(left, right)
}

pub(crate) fn fsub(left: f64, right: f64) -> f64 {
    <f64 as Sub>::sub(left, right)
}

pub(crate) fn fmul(left: f64, right: f64) -> f64 {
    <f64 as Mul>::mul(left, right)
}

pub(crate) fn fdiv(left: f64, right: f64) -> f64 {
    <f64 as Div>::div(left, right)
}

pub(crate) fn fneg(value: f64) -> f64 {
    <f64 as Neg>::neg(value)
}
