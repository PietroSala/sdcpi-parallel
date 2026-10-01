//! The arithmetic of the search, behind one trait with two implementations.
//!
//! `f64` is the arithmetic the recorded runs of the journal paper were produced
//! with, and stays the default of `determine`: nothing in its code path changes,
//! the trait being monomorphised away. `Exact` is arithmetic on rationals of
//! arbitrary precision, for the frontier command, where every comparison with a
//! budget must be exact and every reported value a rational: the probabilities
//! of the instances carry six decimals, so the lattice every achievable value
//! lives on has a denominator of ten to the sixth power per nature node, far
//! past any fixed-width integer, and the honest exact arithmetic is the one
//! that grows as it needs to.
//!
//! What the two share is the shape of the computation, not the numbers: an
//! exact run and a floating-point run of the same search take the same steps,
//! and differ only where a floating-point comparison falls on the wrong side of
//! an equality that holds on paper.

use std::fmt;

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};

use crate::tree::Tree;

pub trait Num: Clone + Send + Sync + fmt::Debug + 'static {
    /// The probability carried along a history.
    type Prob: Clone + Send + Sync + fmt::Debug + 'static;

    fn zero() -> Self;
    fn add(&self, other: &Self) -> Self;
    fn sub(&self, other: &Self) -> Self;
    fn max(&self, other: &Self) -> Self;
    fn min(&self, other: &Self) -> Self;
    /// `p * self`.
    fn scale(&self, p: &Self::Prob) -> Self;
    /// `p * a + (1 - p) * b`, the expectation at a nature node.
    fn expect(p: &Self::Prob, a: &Self, b: &Self) -> Self;
    /// `a <= b` componentwise; `epsilon` is a relative slack that only the
    /// floating-point arithmetic reads.
    fn leq(a: &[Self], b: &[Self], epsilon: f64) -> bool;
    fn to_f64(&self) -> f64;
    /// The value for printing, exact where the arithmetic is.
    fn describe(&self) -> String;

    fn prob_one() -> Self::Prob;
    fn prob_mul(a: &Self::Prob, b: &Self::Prob) -> Self::Prob;
    /// `1 - p`.
    fn prob_complement(p: &Self::Prob) -> Self::Prob;
    fn prob_to_f64(p: &Self::Prob) -> f64;

    /// The impact of the task `id`, one value per component.
    fn impact_of(tree: &Tree, id: u32) -> Vec<Self>;
    /// The probability of the low branch of the nature node `id`.
    fn prob_of(tree: &Tree, id: u32) -> Self::Prob;
    /// A value from its decimal text, as a budget is written.
    fn from_decimal(text: &str) -> Option<Self>;
    /// A value from a whole number: the advance of a fast forward, which the
    /// component zero of the cost pays when the time is in.
    fn from_u32(n: u32) -> Self;
}

#[inline]
fn add_vec<N: Num>(a: &mut [N], b: &[N]) {
    for (x, y) in a.iter_mut().zip(b) {
        *x = x.add(y);
    }
}

/// `a += b`, componentwise.
#[inline]
pub fn add_into<N: Num>(a: &mut [N], b: &[N]) {
    add_vec(a, b);
}

/// `a -= b`, componentwise.
#[inline]
pub fn sub_into<N: Num>(a: &mut [N], b: &[N]) {
    for (x, y) in a.iter_mut().zip(b) {
        *x = x.sub(y);
    }
}

/// `k` zeros.
#[inline]
pub fn zeros<N: Num>(k: usize) -> Vec<N> {
    (0..k).map(|_| N::zero()).collect()
}

// ---------------------------------------------------------------------------
// f64: the arithmetic of the recorded runs
// ---------------------------------------------------------------------------

impl Num for f64 {
    type Prob = f64;

    #[inline]
    fn zero() -> f64 {
        0.0
    }
    #[inline]
    fn add(&self, other: &f64) -> f64 {
        self + other
    }
    #[inline]
    fn sub(&self, other: &f64) -> f64 {
        self - other
    }
    #[inline]
    fn max(&self, other: &f64) -> f64 {
        f64::max(*self, *other)
    }
    #[inline]
    fn min(&self, other: &f64) -> f64 {
        f64::min(*self, *other)
    }
    #[inline]
    fn scale(&self, p: &f64) -> f64 {
        p * self
    }
    #[inline]
    fn expect(p: &f64, a: &f64, b: &f64) -> f64 {
        p * a + (1.0 - p) * b
    }
    #[inline]
    fn leq(a: &[f64], b: &[f64], epsilon: f64) -> bool {
        if epsilon == 0.0 {
            return a.iter().zip(b).all(|(x, y)| x <= y);
        }
        a.iter()
            .zip(b)
            .all(|(x, y)| *x <= *y + epsilon * y.abs().max(1.0))
    }
    #[inline]
    fn to_f64(&self) -> f64 {
        *self
    }
    fn describe(&self) -> String {
        format!("{self}")
    }

    #[inline]
    fn prob_one() -> f64 {
        1.0
    }
    #[inline]
    fn prob_mul(a: &f64, b: &f64) -> f64 {
        a * b
    }
    #[inline]
    fn prob_complement(p: &f64) -> f64 {
        1.0 - p
    }
    #[inline]
    fn prob_to_f64(p: &f64) -> f64 {
        *p
    }

    #[inline]
    fn impact_of(tree: &Tree, id: u32) -> Vec<f64> {
        tree.impact(id).to_vec()
    }
    #[inline]
    fn prob_of(tree: &Tree, id: u32) -> f64 {
        tree.node(id).prob
    }
    fn from_decimal(text: &str) -> Option<f64> {
        text.trim().parse().ok()
    }
    #[inline]
    fn from_u32(n: u32) -> f64 {
        n as f64
    }
}

// ---------------------------------------------------------------------------
// Exact: rationals of arbitrary precision
// ---------------------------------------------------------------------------

/// A rational of arbitrary precision, wrapped so that the trait can be
/// implemented here.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Exact(pub BigRational);

impl Exact {
    pub fn rational(&self) -> &BigRational {
        &self.0
    }
}

/// Parses a decimal such as `0.317537`, `12`, `1e-06` or `-3.5E2` exactly.
/// A number written as a decimal, or as a fraction `a/b` of two integers:
/// the page sends the exact value of a point as a bound, so that the point
/// stays within its own window, which six decimals cannot promise.
pub fn parse_rational(text: &str) -> Option<BigRational> {
    let text = text.trim();
    if let Some((n, d)) = text.split_once('/') {
        let n: num_bigint::BigInt = n.trim().parse().ok()?;
        let d: num_bigint::BigInt = d.trim().parse().ok()?;
        if d.sign() == num_bigint::Sign::NoSign {
            return None;
        }
        return Some(BigRational::new(n, d));
    }
    parse_decimal(text)
}

pub fn parse_decimal(text: &str) -> Option<BigRational> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let (mantissa, exponent) = match text.find(['e', 'E']) {
        Some(i) => (&text[..i], text[i + 1..].parse::<i64>().ok()?),
        None => (text, 0i64),
    };
    let (sign, digits) = match mantissa.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, mantissa.strip_prefix('+').unwrap_or(mantissa)),
    };
    let (int_part, frac_part) = match digits.split_once('.') {
        Some((a, b)) => (a, b),
        None => (digits, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    if !int_part.chars().all(|c| c.is_ascii_digit()) || !frac_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let all: String = format!("{int_part}{frac_part}");
    let all = if all.is_empty() { "0".to_string() } else { all };
    let mut numerator: BigInt = all.parse().ok()?;
    if sign < 0 {
        numerator = -numerator;
    }
    let shift = exponent - frac_part.len() as i64;
    let ten = BigInt::from(10);
    let value = if shift >= 0 {
        BigRational::from_integer(numerator * num_traits::pow(ten, shift as usize))
    } else {
        BigRational::new(numerator, num_traits::pow(ten, (-shift) as usize))
    };
    Some(value)
}

/// A decimal expansion of a rational, exact when it terminates within `digits`
/// places and rounded otherwise.
pub fn decimal(r: &BigRational, digits: usize) -> String {
    let negative = r.is_negative();
    let abs = r.abs();
    let scale = num_traits::pow(BigInt::from(10), digits);
    let scaled = (abs.numer() * &scale) / abs.denom();
    let int_part = &scaled / &scale;
    let frac_part = &scaled % &scale;
    let mut frac = format!("{frac_part:0>width$}", width = digits);
    while frac.ends_with('0') {
        frac.pop();
    }
    let sign = if negative { "-" } else { "" };
    if frac.is_empty() {
        format!("{sign}{int_part}")
    } else {
        format!("{sign}{int_part}.{frac}")
    }
}

impl Num for Exact {
    type Prob = BigRational;

    fn zero() -> Exact {
        Exact(BigRational::zero())
    }
    fn add(&self, other: &Exact) -> Exact {
        Exact(&self.0 + &other.0)
    }
    fn sub(&self, other: &Exact) -> Exact {
        Exact(&self.0 - &other.0)
    }
    fn max(&self, other: &Exact) -> Exact {
        if self.0 >= other.0 {
            self.clone()
        } else {
            other.clone()
        }
    }
    fn min(&self, other: &Exact) -> Exact {
        if self.0 <= other.0 {
            self.clone()
        } else {
            other.clone()
        }
    }
    fn scale(&self, p: &BigRational) -> Exact {
        Exact(p * &self.0)
    }
    fn expect(p: &BigRational, a: &Exact, b: &Exact) -> Exact {
        let q = BigRational::one() - p;
        Exact(p * &a.0 + q * &b.0)
    }
    fn leq(a: &[Exact], b: &[Exact], _epsilon: f64) -> bool {
        a.iter().zip(b).all(|(x, y)| x.0 <= y.0)
    }
    fn to_f64(&self) -> f64 {
        self.0.to_f64().unwrap_or(f64::NAN)
    }
    fn describe(&self) -> String {
        format!("{} ({})", self.0, decimal(&self.0, 12))
    }

    fn prob_one() -> BigRational {
        BigRational::one()
    }
    fn prob_mul(a: &BigRational, b: &BigRational) -> BigRational {
        a * b
    }
    fn prob_complement(p: &BigRational) -> BigRational {
        BigRational::one() - p
    }
    fn prob_to_f64(p: &BigRational) -> f64 {
        p.to_f64().unwrap_or(f64::NAN)
    }

    fn impact_of(tree: &Tree, id: u32) -> Vec<Exact> {
        tree.impact_exact(id).iter().map(|r| Exact(r.clone())).collect()
    }
    fn prob_of(tree: &Tree, id: u32) -> BigRational {
        tree.prob_exact(id).clone()
    }
    fn from_decimal(text: &str) -> Option<Exact> {
        parse_decimal(text).map(Exact)
    }
    fn from_u32(n: u32) -> Exact {
        Exact(BigRational::from_integer(BigInt::from(n)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_parse_exactly() {
        let r = parse_decimal("0.317537").unwrap();
        assert_eq!(r, BigRational::new(BigInt::from(317537), BigInt::from(1_000_000)));
        let r = parse_decimal("1e-06").unwrap();
        assert_eq!(r, BigRational::new(BigInt::from(1), BigInt::from(1_000_000)));
        let r = parse_decimal("12").unwrap();
        assert_eq!(r, BigRational::from_integer(BigInt::from(12)));
        assert_eq!(decimal(&parse_decimal("2.50").unwrap(), 6), "2.5");
    }
}
