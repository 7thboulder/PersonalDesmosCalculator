//! Exact symbolic expressions and the simplification that keeps them in
//! one canonical form: sums and products are flat and sorted, like terms
//! and like powers are combined, and numbers stay exact fractions for as
//! long as they fit.

use std::cmp::Ordering;
use std::f64::consts::PI;

/// An exact fraction, always in lowest terms with a positive denominator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Q {
    pub n: i128,
    pub d: i128,
}

/// Fractions bigger than this become decimals, so arithmetic cannot overflow.
const LIMIT: i128 = 1 << 100;

pub fn gcd(a: i128, b: i128) -> i128 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

impl Q {
    pub const ZERO: Q = Q { n: 0, d: 1 };
    pub const ONE: Q = Q { n: 1, d: 1 };

    pub fn new(n: i128, d: i128) -> Option<Q> {
        if d == 0 {
            return None;
        }
        let g = gcd(n, d).max(1);
        let s = if d < 0 { -1 } else { 1 };
        let (n, d) = (s * n / g, s * d / g);
        (n.abs() <= LIMIT && d <= LIMIT).then_some(Q { n, d })
    }

    pub fn int(n: i128) -> Q {
        Q { n, d: 1 }
    }

    pub fn add(self, o: Q) -> Option<Q> {
        Q::new(
            self.n
                .checked_mul(o.d)?
                .checked_add(o.n.checked_mul(self.d)?)?,
            self.d.checked_mul(o.d)?,
        )
    }

    pub fn sub(self, o: Q) -> Option<Q> {
        self.add(o.neg())
    }

    pub fn mul(self, o: Q) -> Option<Q> {
        Q::new(self.n.checked_mul(o.n)?, self.d.checked_mul(o.d)?)
    }

    pub fn div(self, o: Q) -> Option<Q> {
        if o.n == 0 {
            return None;
        }
        Q::new(self.n.checked_mul(o.d)?, self.d.checked_mul(o.n)?)
    }

    pub fn neg(self) -> Q {
        Q {
            n: -self.n,
            d: self.d,
        }
    }

    pub fn abs(self) -> Q {
        Q {
            n: self.n.abs(),
            d: self.d,
        }
    }

    pub fn is_int(self) -> bool {
        self.d == 1
    }

    pub fn is_zero(self) -> bool {
        self.n == 0
    }

    pub fn to_f64(self) -> f64 {
        self.n as f64 / self.d as f64
    }

    /// The fraction a decimal was typed as: 0.1 is 1/10, not the nearest
    /// binary fraction.
    pub fn from_f64(v: f64) -> Option<Q> {
        if !v.is_finite() {
            return None;
        }
        if v.fract() == 0.0 && v.abs() < 1e18 {
            return Some(Q::int(v as i128));
        }
        let mut scale: i128 = 1;
        for _ in 0..15 {
            scale *= 10;
            let m = (v * scale as f64).round();
            if m.abs() < 1e27 && m / scale as f64 == v {
                return Q::new(m as i128, scale);
            }
        }
        None
    }

    pub fn pow(self, e: i64) -> Option<Q> {
        if e.unsigned_abs() > 4096 {
            return None;
        }
        let mut out = Q::ONE;
        for _ in 0..e.unsigned_abs() {
            out = out.mul(self)?;
        }
        if e < 0 {
            Q::ONE.div(out)
        } else {
            Some(out)
        }
    }

    pub fn cmp_zero(self) -> Ordering {
        self.n.cmp(&0)
    }
}

impl PartialOrd for Q {
    fn partial_cmp(&self, o: &Q) -> Option<Ordering> {
        Some((self.n * o.d).cmp(&(o.n * self.d)))
    }
}

/// The functions symbolic work understands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Func {
    Sin,
    Cos,
    Tan,
    Csc,
    Sec,
    Cot,
    Asin,
    Acos,
    Atan,
    Sinh,
    Cosh,
    Tanh,
    Asinh,
    Acosh,
    Atanh,
    Ln,
    Log,
    Abs,
}

impl Func {
    pub fn eval(self, x: f64) -> f64 {
        match self {
            Func::Sin => x.sin(),
            Func::Cos => x.cos(),
            Func::Tan => x.tan(),
            Func::Csc => 1.0 / x.sin(),
            Func::Sec => 1.0 / x.cos(),
            Func::Cot => 1.0 / x.tan(),
            Func::Asin => x.asin(),
            Func::Acos => x.acos(),
            Func::Atan => x.atan(),
            Func::Sinh => x.sinh(),
            Func::Cosh => x.cosh(),
            Func::Tanh => x.tanh(),
            Func::Asinh => x.asinh(),
            Func::Acosh => x.acosh(),
            Func::Atanh => x.atanh(),
            Func::Ln => x.ln(),
            Func::Log => x.log10(),
            Func::Abs => x.abs(),
        }
    }

    fn odd(self) -> bool {
        use Func::*;
        matches!(
            self,
            Sin | Tan | Csc | Cot | Asin | Atan | Sinh | Tanh | Asinh | Atanh
        )
    }

    fn even(self) -> bool {
        matches!(self, Func::Cos | Func::Sec | Func::Cosh | Func::Abs)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Sym {
    Num(Q),
    /// A number that is no longer exact.
    Real(f64),
    /// A name, including the constants `pi` and `e` and the imaginary `i`.
    Var(String),
    Add(Vec<Sym>),
    Mul(Vec<Sym>),
    Pow(Box<Sym>, Box<Sym>),
    Fn(Func, Box<Sym>),
}

pub fn n(v: i128) -> Sym {
    Sym::Num(Q::int(v))
}

pub fn frac(a: i128, b: i128) -> Sym {
    match Q::new(a, b) {
        Some(q) => Sym::Num(q),
        None => Sym::Real(a as f64 / b as f64),
    }
}

pub fn var(name: &str) -> Sym {
    Sym::Var(name.to_owned())
}

pub fn pi() -> Sym {
    var("pi")
}

pub fn num_f64(v: f64) -> Sym {
    match Q::from_f64(v) {
        Some(q) => Sym::Num(q),
        None => Sym::Real(v),
    }
}

/// Whether a name is a constant rather than an unknown.
pub fn is_constant_name(name: &str) -> bool {
    matches!(name, "pi" | "e" | "i")
}

impl Sym {
    pub fn as_q(&self) -> Option<Q> {
        match self {
            Sym::Num(q) => Some(*q),
            _ => None,
        }
    }

    pub fn is_num(&self) -> bool {
        matches!(self, Sym::Num(_) | Sym::Real(_))
    }

    pub fn is_zero(&self) -> bool {
        match self {
            Sym::Num(q) => q.is_zero(),
            Sym::Real(r) => *r == 0.0,
            _ => false,
        }
    }

    pub fn is_one(&self) -> bool {
        matches!(self, Sym::Num(q) if *q == Q::ONE)
    }

    pub fn is_int(&self, v: i128) -> bool {
        matches!(self, Sym::Num(q) if *q == Q::int(v))
    }

    /// Whether `name` appears anywhere inside.
    pub fn has(&self, name: &str) -> bool {
        match self {
            Sym::Num(_) | Sym::Real(_) => false,
            Sym::Var(v) => v == name,
            Sym::Add(xs) | Sym::Mul(xs) => xs.iter().any(|x| x.has(name)),
            Sym::Pow(a, b) => a.has(name) || b.has(name),
            Sym::Fn(_, a) => a.has(name),
        }
    }

    /// Whether this contains `target` as a whole sub-expression.
    pub fn contains(&self, target: &Sym) -> bool {
        if self == target {
            return true;
        }
        match self {
            Sym::Add(xs) | Sym::Mul(xs) => xs.iter().any(|x| x.contains(target)),
            Sym::Pow(a, b) => a.contains(target) || b.contains(target),
            Sym::Fn(_, a) => a.contains(target),
            _ => false,
        }
    }

    /// The unknowns this reads, in order of first appearance.
    pub fn names(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_names(&mut out);
        out
    }

    fn collect_names(&self, out: &mut Vec<String>) {
        match self {
            Sym::Var(v) if !is_constant_name(v) => {
                if !out.contains(v) {
                    out.push(v.clone())
                }
            }
            Sym::Add(xs) | Sym::Mul(xs) => xs.iter().for_each(|x| x.collect_names(out)),
            Sym::Pow(a, b) => {
                a.collect_names(out);
                b.collect_names(out);
            }
            Sym::Fn(_, a) => a.collect_names(out),
            _ => {}
        }
    }

    /// Whether this is a plain number once the constants are counted as
    /// numbers: 2, √3, π/4, ln 2.
    pub fn is_constant(&self) -> bool {
        self.names().is_empty() && !self.has("i")
    }

    /// Replaces every `target` with `with`, simplifying as it goes.
    pub fn subs(&self, target: &Sym, with: &Sym) -> Sym {
        if self == target {
            return with.clone();
        }
        match self {
            Sym::Add(xs) => add(xs.iter().map(|x| x.subs(target, with)).collect()),
            Sym::Mul(xs) => mul(xs.iter().map(|x| x.subs(target, with)).collect()),
            Sym::Pow(a, b) => pow(a.subs(target, with), b.subs(target, with)),
            Sym::Fn(f, a) => func(*f, a.subs(target, with)),
            other => other.clone(),
        }
    }

    pub fn subs_var(&self, name: &str, with: &Sym) -> Sym {
        self.subs(&var(name), with)
    }

    /// The value, with every unknown looked up in `env`.
    pub fn eval(&self, env: &dyn Fn(&str) -> Option<f64>) -> f64 {
        match self {
            Sym::Num(q) => q.to_f64(),
            Sym::Real(r) => *r,
            Sym::Var(v) => match v.as_str() {
                "pi" => PI,
                "e" => std::f64::consts::E,
                name => env(name).unwrap_or(f64::NAN),
            },
            Sym::Add(xs) => xs.iter().map(|x| x.eval(env)).sum(),
            Sym::Mul(xs) => xs.iter().map(|x| x.eval(env)).product(),
            Sym::Pow(a, b) => crate::builtins::pow(a.eval(env), b.eval(env)),
            Sym::Fn(f, a) => f.eval(a.eval(env)),
        }
    }

    /// The value of a constant expression.
    pub fn value(&self) -> f64 {
        self.eval(&|_| None)
    }

    /// The value with `name` set to `x`.
    pub fn at(&self, name: &str, x: f64) -> f64 {
        self.eval(&|n| (n == name).then_some(x))
    }

    /// How many nodes this has, as a measure of how complicated it is.
    pub fn size(&self) -> usize {
        match self {
            Sym::Add(xs) | Sym::Mul(xs) => 1 + xs.iter().map(Sym::size).sum::<usize>(),
            Sym::Pow(a, b) => 1 + a.size() + b.size(),
            Sym::Fn(_, a) => 1 + a.size(),
            _ => 1,
        }
    }

    /// Whether this reads as negative: -3, -2x.
    pub fn is_negative(&self) -> bool {
        match self {
            Sym::Num(q) => q.n < 0,
            Sym::Real(r) => *r < 0.0,
            Sym::Mul(xs) => xs.first().is_some_and(Sym::is_negative),
            _ => false,
        }
    }

    /// The terms of a sum, or this alone.
    pub fn terms(&self) -> Vec<Sym> {
        match self {
            Sym::Add(xs) => xs.clone(),
            other => vec![other.clone()],
        }
    }

    /// The factors of a product, or this alone.
    pub fn factors(&self) -> Vec<Sym> {
        match self {
            Sym::Mul(xs) => xs.clone(),
            other => vec![other.clone()],
        }
    }
}

// ---- Arithmetic on numbers ----------------------------------------------------

fn num_add(a: &Sym, b: &Sym) -> Sym {
    match (a, b) {
        (Sym::Num(x), Sym::Num(y)) => match x.add(*y) {
            Some(q) => Sym::Num(q),
            None => Sym::Real(x.to_f64() + y.to_f64()),
        },
        _ => Sym::Real(a.value() + b.value()),
    }
}

fn num_mul(a: &Sym, b: &Sym) -> Sym {
    match (a, b) {
        (Sym::Num(x), Sym::Num(y)) => match x.mul(*y) {
            Some(q) => Sym::Num(q),
            None => Sym::Real(x.to_f64() * y.to_f64()),
        },
        _ => Sym::Real(a.value() * b.value()),
    }
}

/// A term as its number and the rest: 3x² is 3 and x².
pub fn split_coef(t: &Sym) -> (Sym, Sym) {
    match t {
        Sym::Num(_) | Sym::Real(_) => (t.clone(), n(1)),
        Sym::Mul(xs) if xs[0].is_num() => {
            let rest = if xs.len() == 2 {
                xs[1].clone()
            } else {
                Sym::Mul(xs[1..].to_vec())
            };
            (xs[0].clone(), rest)
        }
        _ => (n(1), t.clone()),
    }
}

/// `coef` times an already simplified `rest`, which may be a product.
fn with_coef(coef: Sym, rest: Sym) -> Sym {
    if coef.is_one() {
        return rest;
    }
    if rest.is_one() {
        return coef;
    }
    match rest {
        Sym::Mul(mut xs) => {
            xs.insert(0, coef);
            Sym::Mul(xs)
        }
        other => Sym::Mul(vec![coef, other]),
    }
}

// ---- Ordering -------------------------------------------------------------------

/// How a term counts toward the degree of a polynomial.
pub fn degree(t: &Sym) -> f64 {
    match t {
        Sym::Var(v) if !is_constant_name(v) => 1.0,
        Sym::Mul(xs) => xs.iter().map(degree).sum(),
        Sym::Pow(b, e) => match e.as_q() {
            Some(q) => degree(b) * q.to_f64(),
            None => 0.0,
        },
        Sym::Add(xs) => xs.iter().map(degree).fold(0.0, f64::max),
        _ => 0.0,
    }
}

/// The exponent of each unknown in a term.
fn exponents(t: &Sym, out: &mut Vec<(String, f64)>, scale: f64) {
    match t {
        Sym::Var(v) if !is_constant_name(v) => out.push((v.clone(), scale)),
        Sym::Mul(xs) => xs.iter().for_each(|x| exponents(x, out, scale)),
        Sym::Pow(b, e) => {
            if let Some(q) = e.as_q() {
                exponents(b, out, scale * q.to_f64())
            }
        }
        _ => {}
    }
}

fn exponent_of(list: &[(String, f64)], name: &str) -> f64 {
    list.iter().filter(|(n, _)| n == name).map(|(_, e)| e).sum()
}

fn key(s: &Sym) -> String {
    format!("{s:?}")
}

/// Terms of a sum: highest degree first, then x before y, numbers last.
fn term_order(a: &Sym, b: &Sym) -> Ordering {
    let (da, db) = (degree(a), degree(b));
    if da != db {
        return db.partial_cmp(&da).unwrap_or(Ordering::Equal);
    }
    let (mut ea, mut eb) = (Vec::new(), Vec::new());
    exponents(a, &mut ea, 1.0);
    exponents(b, &mut eb, 1.0);
    let mut names: Vec<&String> = ea.iter().chain(eb.iter()).map(|(n, _)| n).collect();
    names.sort();
    names.dedup();
    for name in names {
        let (x, y) = (exponent_of(&ea, name), exponent_of(&eb, name));
        if x != y {
            return y.partial_cmp(&x).unwrap_or(Ordering::Equal);
        }
    }
    let (_, ra) = split_coef(a);
    let (_, rb) = split_coef(b);
    key(&ra).cmp(&key(&rb)).then_with(|| key(a).cmp(&key(b)))
}

/// Where a factor sits in a product: numbers, roots of numbers, π and e,
/// letters, then functions and brackets.
fn factor_class(f: &Sym) -> u8 {
    match f {
        Sym::Num(_) | Sym::Real(_) => 0,
        Sym::Pow(b, _) if b.is_num() => 1,
        Sym::Var(v) if v == "pi" || v == "e" => 2,
        Sym::Pow(b, e) if matches!(&**b, Sym::Var(v) if v == "e") && !e.is_constant() => 6,
        Sym::Var(_) => 3,
        Sym::Pow(b, _) => factor_class(b).max(3),
        Sym::Fn(..) => 5,
        Sym::Add(_) => 7,
        Sym::Mul(_) => 8,
    }
}

fn factor_order(a: &Sym, b: &Sym) -> Ordering {
    let base = |s: &Sym| match s {
        Sym::Pow(b, _) => (**b).clone(),
        other => other.clone(),
    };
    factor_class(a)
        .cmp(&factor_class(b))
        .then_with(|| key(&base(a)).cmp(&key(&base(b))))
        .then_with(|| key(a).cmp(&key(b)))
}

// ---- Simplifying constructors -----------------------------------------------------

pub fn add(terms: Vec<Sym>) -> Sym {
    let mut flat = Vec::new();
    for t in terms {
        match t {
            Sym::Add(xs) => flat.extend(xs),
            t => flat.push(t),
        }
    }
    let mut constant = n(0);
    let mut parts: Vec<(Sym, Sym)> = Vec::new();
    for t in flat {
        if t.is_num() {
            constant = num_add(&constant, &t);
            continue;
        }
        let (c, rest) = split_coef(&t);
        match parts.iter_mut().find(|(r, _)| *r == rest) {
            Some(p) => p.1 = num_add(&p.1, &c),
            None => parts.push((rest, c)),
        }
    }
    let mut out: Vec<Sym> = parts
        .into_iter()
        .filter(|(_, c)| !c.is_zero())
        .map(|(r, c)| with_coef(c, r))
        .collect();
    out.sort_by(term_order);
    if !constant.is_zero() {
        out.push(constant);
    }
    out = sin_cos_squares(out);
    match out.len() {
        0 => n(0),
        1 => out.pop().unwrap(),
        _ => Sym::Add(out),
    }
}

/// c·sin²u + c·cos²u = c, wherever both appear in a sum.
fn sin_cos_squares(terms: Vec<Sym>) -> Vec<Sym> {
    // The term as (everything else, u) when it has a factor f(u)².
    let square_of = |t: &Sym, f: Func| -> Option<(Sym, Sym)> {
        let fs = t.factors();
        let i = fs.iter().position(|x| match x {
            Sym::Pow(b, e) if e.is_int(2) => matches!(&**b, Sym::Fn(g, _) if *g == f),
            _ => false,
        })?;
        let Sym::Pow(b, _) = &fs[i] else { return None };
        let Sym::Fn(_, u) = &**b else { return None };
        let others: Vec<Sym> = fs
            .iter()
            .enumerate()
            .filter(|(k, _)| *k != i)
            .map(|(_, x)| x.clone())
            .collect();
        Some((mul(others), (**u).clone()))
    };
    for (i, t) in terms.iter().enumerate() {
        if let Some((c, u)) = square_of(t, Func::Sin) {
            if let Some(j) = terms
                .iter()
                .position(|s| square_of(s, Func::Cos) == Some((c.clone(), u.clone())))
            {
                let mut rest: Vec<Sym> = terms
                    .iter()
                    .enumerate()
                    .filter(|(k, _)| *k != i && *k != j)
                    .map(|(_, s)| s.clone())
                    .collect();
                rest.push(c);
                return add(rest).terms();
            }
        }
    }
    terms
}

pub fn add2(a: Sym, b: Sym) -> Sym {
    add(vec![a, b])
}

pub fn sub(a: Sym, b: Sym) -> Sym {
    add(vec![a, neg(b)])
}

pub fn neg(a: Sym) -> Sym {
    mul(vec![n(-1), a])
}

pub fn mul(factors: Vec<Sym>) -> Sym {
    let mut flat = Vec::new();
    for f in factors {
        match f {
            Sym::Mul(xs) => flat.extend(xs),
            f => flat.push(f),
        }
    }
    let mut coef = n(1);
    let mut bases: Vec<(Sym, Sym)> = Vec::new();
    for f in flat {
        if f.is_num() {
            coef = num_mul(&coef, &f);
            continue;
        }
        let (b, e) = match f {
            Sym::Pow(b, e) => (*b, *e),
            other => (other, n(1)),
        };
        match bases.iter_mut().find(|(x, _)| *x == b) {
            Some(p) => p.1 = add2(p.1.clone(), e),
            None => bases.push((b, e)),
        }
    }
    if coef.is_zero() {
        return coef;
    }
    let mut out = Vec::new();
    let mut again = false;
    for (b, e) in bases {
        let p = pow(b, e);
        match p {
            p if p.is_num() => coef = num_mul(&coef, &p),
            Sym::Mul(xs) => {
                again = true;
                out.extend(xs)
            }
            p if p.is_one() => {}
            p => out.push(p),
        }
    }
    if again {
        out.push(coef);
        return mul(out);
    }
    out.sort_by(factor_order);
    if out.is_empty() {
        return coef;
    }
    if !coef.is_one() {
        out.insert(0, coef);
    }
    if out.len() == 1 {
        out.pop().unwrap()
    } else {
        Sym::Mul(out)
    }
}

pub fn mul2(a: Sym, b: Sym) -> Sym {
    mul(vec![a, b])
}

pub fn div(a: Sym, b: Sym) -> Sym {
    mul(vec![a, pow(b, n(-1))])
}

pub fn powi(a: Sym, e: i128) -> Sym {
    pow(a, n(e))
}

pub fn sqrt(a: Sym) -> Sym {
    pow(a, frac(1, 2))
}

pub fn exp(a: Sym) -> Sym {
    pow(var("e"), a)
}

pub fn pow(b: Sym, e: Sym) -> Sym {
    if e.is_zero() {
        return n(1);
    }
    if e.is_one() {
        return b;
    }
    if b.is_one() {
        return b;
    }
    match (&b, &e) {
        (Sym::Num(x), Sym::Num(q)) => return pow_q(*x, *q),
        (Sym::Real(_), _) | (_, Sym::Real(_)) if b.is_num() && e.is_num() => {
            return Sym::Real(crate::builtins::pow(b.value(), e.value()))
        }
        _ => {}
    }
    if b.is_zero() {
        return match e.as_q() {
            Some(q) if q.n > 0 => n(0),
            _ => Sym::Pow(Box::new(b), Box::new(e)),
        };
    }
    // i² = -1.
    if let (Sym::Var(name), Some(q)) = (&b, e.as_q()) {
        if name == "i" && q.is_int() {
            return match q.n.rem_euclid(4) {
                0 => n(1),
                1 => var("i"),
                2 => n(-1),
                _ => neg(var("i")),
            };
        }
    }
    // e^(ln u) = u.
    if matches!(&b, Sym::Var(v) if v == "e") {
        if let Sym::Fn(Func::Ln, u) = &e {
            return (**u).clone();
        }
    }
    // 10^(log u) = u.
    if b.is_int(10) {
        if let Sym::Fn(Func::Log, u) = &e {
            return (**u).clone();
        }
    }
    match b {
        // (e^u)^k = e^(ku) for real u.
        Sym::Pow(inner, e2) if *inner == var("e") && !e2.has("i") => pow(*inner, mul2(*e2, e)),
        Sym::Pow(inner, e2) => {
            if e.as_q().is_some_and(Q::is_int) {
                return pow(*inner, mul2(*e2, e));
            }
            // √(x²) = |x|.
            if let (Some(a), Some(c)) = (e2.as_q(), e.as_q()) {
                if a.is_int() && a.n % 2 == 0 && a.mul(c) == Some(Q::ONE) {
                    return func(Func::Abs, *inner);
                }
                if a.is_int() && a.n % 2 == 1 {
                    return pow(*inner, mul2(*e2, e));
                }
            }
            Sym::Pow(Box::new(Sym::Pow(inner, e2)), Box::new(e))
        }
        Sym::Mul(xs) => {
            if e.as_q().is_some_and(Q::is_int) {
                return mul(xs.into_iter().map(|x| pow(x, e.clone())).collect());
            }
            // √(4x) = 2√x, taking out the positive numbers.
            let (inside, outside): (Vec<Sym>, Vec<Sym>) = xs
                .into_iter()
                .partition(|x| !(x.is_constant() && x.value() > 0.0));
            if outside.is_empty() {
                return Sym::Pow(Box::new(Sym::Mul(inside)), Box::new(e));
            }
            let mut parts: Vec<Sym> = outside.into_iter().map(|x| pow(x, e.clone())).collect();
            parts.push(match inside.len() {
                0 => n(1),
                1 => Sym::Pow(Box::new(inside[0].clone()), Box::new(e)),
                _ => Sym::Pow(Box::new(Sym::Mul(inside)), Box::new(e)),
            });
            mul(parts)
        }
        b => Sym::Pow(Box::new(b), Box::new(e)),
    }
}

/// The largest m with m^k dividing v, for small enough v.
fn kth_power_part(v: i128, k: i128) -> (i128, i128) {
    if v.abs() > 1 << 62 || k > 64 {
        return (1, v);
    }
    let mut out = 1i128;
    let mut rest = v;
    let mut p = 2i128;
    while p.checked_pow(k as u32).is_some_and(|pk| pk <= rest.abs()) {
        let pk = p.pow(k as u32);
        while rest % pk == 0 {
            rest /= pk;
            out *= p;
        }
        p += 1;
        if p > 1_000_000 {
            break;
        }
    }
    (out, rest)
}

fn pow_q(x: Q, e: Q) -> Sym {
    let real = || Sym::Real(crate::builtins::pow(x.to_f64(), e.to_f64()));
    if e.is_int() {
        if x.is_zero() && e.n < 0 {
            return Sym::Pow(Box::new(Sym::Num(x)), Box::new(Sym::Num(e)));
        }
        return match x.pow(e.n as i64) {
            Some(q) => Sym::Num(q),
            None => real(),
        };
    }
    if x.is_zero() {
        return if e.n > 0 {
            n(0)
        } else {
            Sym::Pow(Box::new(Sym::Num(x)), Box::new(Sym::Num(e)))
        };
    }
    let k = e.d;
    if x.n < 0 {
        // Odd roots of negatives are negative; even ones are left alone.
        if k % 2 == 1 {
            let positive = pow_q(x.neg(), e);
            return if e.n % 2 == 0 {
                positive
            } else {
                neg(positive)
            };
        }
        return Sym::Pow(Box::new(Sym::Num(x)), Box::new(Sym::Num(e)));
    }
    // x^(w + p/k) = x^w · (x^p)^(1/k).
    let w = e.n.div_euclid(e.d);
    let p = e.n.rem_euclid(e.d);
    let Some(whole) = x.pow(w as i64) else {
        return real();
    };
    let Some(xp) = x.pow(p as i64) else {
        return real();
    };
    // Rationalize: (N/D)^(1/k) = (N·D^(k-1))^(1/k) / D.
    let Some(inside) = Q::int(xp.n).mul(Q::int(xp.d).pow(k as i64 - 1).unwrap_or(Q::ZERO)) else {
        return real();
    };
    if inside.is_zero() {
        return real();
    }
    let (out, rest) = kth_power_part(inside.n, k);
    let Some(coef) = whole.mul(Q::int(out)).and_then(|c| c.div(Q::int(xp.d))) else {
        return real();
    };
    if rest == 1 {
        return Sym::Num(coef);
    }
    let root = Sym::Pow(Box::new(n(rest)), Box::new(frac(1, k)));
    if coef == Q::ONE {
        root
    } else {
        Sym::Mul(vec![Sym::Num(coef), root])
    }
}

/// The number c when `s` is c·π.
pub fn pi_multiple(s: &Sym) -> Option<Q> {
    match s {
        Sym::Num(q) if q.is_zero() => Some(Q::ZERO),
        Sym::Var(v) if v == "pi" => Some(Q::ONE),
        Sym::Mul(xs) if xs.len() == 2 && xs[1] == pi() => xs[0].as_q(),
        _ => None,
    }
}

/// sin(c·π) for the angles with known values, c reduced to [0, 2).
fn sin_exact(c: Q) -> Option<Sym> {
    let c = Q::new(c.n.rem_euclid(2 * c.d), c.d)?;
    // Twelfths of a half turn.
    let twelfths = c.mul(Q::int(12))?;
    if !twelfths.is_int() {
        return None;
    }
    let t = twelfths.n;
    let (sign, t) = if t >= 12 { (-1, t - 12) } else { (1, t) };
    let t = if t > 6 { 12 - t } else { t };
    let r2 = || sqrt(n(2));
    let r3 = || sqrt(n(3));
    let v = match t {
        0 => n(0),
        2 => frac(1, 2),
        3 => div(r2(), n(2)),
        4 => div(r3(), n(2)),
        6 => n(1),
        // π/12 and 5π/12 stay as written.
        _ => return None,
    };
    Some(mul2(n(sign), v))
}

/// The angle c·π whose function value is `v`, for the table's angles.
fn angle_for(v: f64, f: impl Fn(f64) -> f64, lo: f64, hi: f64) -> Option<Sym> {
    for d in [1i128, 2, 3, 4, 6] {
        for k in -2 * d..=2 * d {
            let a = k as f64 / d as f64 * PI;
            if a < lo - 1e-12 || a > hi + 1e-12 {
                continue;
            }
            if (f(a) - v).abs() < 1e-12 {
                return Some(mul2(frac(k, d), pi()));
            }
        }
    }
    None
}

pub fn func(f: Func, a: Sym) -> Sym {
    if let Sym::Real(x) = a {
        return Sym::Real(f.eval(x));
    }
    // Odd and even functions take the sign out: sin(-x) = -sin x.
    if a.is_negative() && (f.odd() || f.even()) {
        let inner = neg(a);
        return if f.odd() {
            neg(func(f, inner))
        } else {
            func(f, inner)
        };
    }
    let raw = |a: Sym| Sym::Fn(f, Box::new(a));
    match f {
        Func::Sin | Func::Cos | Func::Tan | Func::Csc | Func::Sec | Func::Cot => {
            if let Some(c) = pi_multiple(&a) {
                let half = Q::new(1, 2).unwrap();
                let s = sin_exact(c);
                let co = c.add(half).and_then(sin_exact);
                let value = match (f, s, co) {
                    (Func::Sin, Some(s), _) => Some(s),
                    (Func::Cos, _, Some(c)) => Some(c),
                    (Func::Tan, Some(s), Some(c)) if !c.is_zero() => Some(div(s, c)),
                    (Func::Cot, Some(s), Some(c)) if !s.is_zero() => Some(div(c, s)),
                    (Func::Csc, Some(s), _) if !s.is_zero() => Some(div(n(1), s)),
                    (Func::Sec, _, Some(c)) if !c.is_zero() => Some(div(n(1), c)),
                    _ => None,
                };
                if let Some(v) = value {
                    return v;
                }
            }
            raw(a)
        }
        Func::Asin | Func::Acos | Func::Atan if a.is_constant() => {
            let v = a.value();
            let found = match f {
                Func::Asin => angle_for(v, f64::sin, -PI / 2.0, PI / 2.0),
                Func::Acos => angle_for(v, f64::cos, 0.0, PI),
                _ => angle_for(v, f64::tan, -PI / 2.0 + 1e-9, PI / 2.0 - 1e-9),
            };
            found.unwrap_or_else(|| raw(a))
        }
        Func::Ln => {
            if a.is_one() {
                return n(0);
            }
            if a == var("e") {
                return n(1);
            }
            if let Sym::Pow(b, e) = &a {
                if **b == var("e") && !e.has("i") {
                    return (**e).clone();
                }
            }
            raw(a)
        }
        Func::Log => {
            if a.is_one() {
                return n(0);
            }
            if let Some(q) = a.as_q() {
                // log 1000 = 3.
                let mut p = 1i128;
                for k in 0..30 {
                    if Q::int(p) == q {
                        return n(k);
                    }
                    p *= 10;
                }
            }
            if let Sym::Pow(b, e) = &a {
                if b.is_int(10) {
                    return (**e).clone();
                }
            }
            raw(a)
        }
        Func::Abs => {
            if let Some(q) = a.as_q() {
                return Sym::Num(q.abs());
            }
            if a.is_constant() && a.value() > 0.0 {
                return a;
            }
            if let Sym::Fn(Func::Abs, _) = a {
                return a;
            }
            // x² + 1 and the like are never negative.
            if let Sym::Add(ts) = &a {
                let positive = |t: &Sym| {
                    let (c, rest) = split_coef(t);
                    c.is_constant()
                        && c.value() > 0.0
                        && rest.factors().iter().all(|f| match f {
                            Sym::Pow(_, k) => k.as_q().is_some_and(|q| q.is_int() && q.n % 2 == 0),
                            f => f.is_one() || (f.is_constant() && f.value() > 0.0),
                        })
                };
                if ts.iter().all(positive) {
                    return a;
                }
            }
            // e^u and even roots are never negative.
            if let Sym::Pow(b, e) = &a {
                let even_root = e.as_q().is_some_and(|q| q.d % 2 == 0);
                if (**b == var("e") && !e.has("i")) || even_root {
                    return a;
                }
            }
            raw(a)
        }
        Func::Sinh | Func::Tanh | Func::Asinh | Func::Atanh if a.is_zero() => n(0),
        Func::Cosh if a.is_zero() => n(1),
        _ => raw(a),
    }
}

// ---- Rewriting --------------------------------------------------------------------

/// Multiplies out products and whole-number powers of sums.
pub fn expand(s: &Sym) -> Sym {
    match s {
        Sym::Add(xs) => add(xs.iter().map(expand).collect()),
        Sym::Mul(xs) => {
            let parts: Vec<Sym> = xs.iter().map(expand).collect();
            distribute(parts)
        }
        Sym::Pow(b, e) => {
            let b = expand(b);
            let e = expand(e);
            if let (Sym::Add(_), Some(q)) = (&b, e.as_q()) {
                if q.is_int() && q.n > 1 && q.n <= 12 {
                    let parts = vec![b.clone(); q.n as usize];
                    return distribute(parts);
                }
            }
            pow(b, e)
        }
        Sym::Fn(f, a) => func(*f, expand(a)),
        other => other.clone(),
    }
}

fn distribute(parts: Vec<Sym>) -> Sym {
    let mut acc = vec![n(1)];
    for p in &parts {
        let terms = match p {
            Sym::Add(ts) => ts.clone(),
            other => vec![other.clone()],
        };
        if acc.len() * terms.len() > 4000 {
            return mul(parts);
        }
        let mut next = Vec::with_capacity(acc.len() * terms.len());
        for a in &acc {
            for t in &terms {
                next.push(mul2(a.clone(), t.clone()));
            }
        }
        acc = next;
    }
    add(acc)
}

/// A product's factors split into the ones on top and the ones below.
pub fn num_den(s: &Sym) -> (Sym, Sym) {
    match s {
        Sym::Num(q) => (n(q.n), n(q.d)),
        Sym::Mul(xs) => {
            let mut top = Vec::new();
            let mut bottom = Vec::new();
            for x in xs {
                let (a, b) = num_den(x);
                top.push(a);
                bottom.push(b);
            }
            (mul(top), mul(bottom))
        }
        Sym::Pow(b, e) if e.is_negative() => {
            let (nb, db) = num_den(b);
            let pe = neg((**e).clone());
            (pow(db, pe.clone()), pow(nb, pe))
        }
        Sym::Pow(b, e) if e.as_q().is_some_and(|q| q.is_int() && q.n > 0) => {
            let (nb, db) = num_den(b);
            (pow(nb, (**e).clone()), pow(db, (**e).clone()))
        }
        Sym::Add(xs) => {
            let parts: Vec<(Sym, Sym)> = xs.iter().map(num_den).collect();
            if parts.iter().all(|(_, d)| d.is_one()) {
                return (s.clone(), n(1));
            }
            let common = lcm_all(parts.iter().map(|(_, d)| d.clone()).collect());
            let top = add(parts
                .into_iter()
                .map(|(a, d)| mul2(a, div(common.clone(), d)))
                .collect());
            (top, common)
        }
        other => (other.clone(), n(1)),
    }
}

/// The least common multiple of products, taking each factor to the
/// highest power it appears with.
fn lcm_all(ds: Vec<Sym>) -> Sym {
    let mut numeric = 1i128;
    let mut bases: Vec<(Sym, Q)> = Vec::new();
    let mut other: Vec<Sym> = Vec::new();
    for d in ds {
        for f in d.factors() {
            match f {
                Sym::Num(q) if q.is_int() => {
                    let g = gcd(numeric, q.n).max(1);
                    numeric = (numeric / g).saturating_mul(q.n.abs());
                }
                Sym::Pow(b, e) if e.as_q().is_some() => {
                    let e = e.as_q().unwrap();
                    match bases.iter_mut().find(|(x, _)| *x == *b) {
                        Some(p) => {
                            if e > p.1 {
                                p.1 = e
                            }
                        }
                        None => bases.push((*b, e)),
                    }
                }
                f if f.is_num() => other.push(f),
                f => match bases.iter_mut().find(|(x, _)| *x == f) {
                    Some(p) => {
                        if Q::ONE > p.1 {
                            p.1 = Q::ONE
                        }
                    }
                    None => bases.push((f, Q::ONE)),
                },
            }
        }
    }
    let mut parts = vec![n(numeric)];
    parts.extend(other);
    parts.extend(bases.into_iter().map(|(b, e)| pow(b, Sym::Num(e))));
    mul(parts)
}

/// The expression as one fraction, with the top multiplied out.
pub fn together(s: &Sym) -> Sym {
    let (top, bottom) = num_den(s);
    div(expand(&top), bottom)
}

/// Replaces `target` with `with`, also where a power of it appears:
/// with target e^x, e^(2x) becomes with².
pub fn replace_power(s: &Sym, target: &Sym, with: &Sym) -> Sym {
    if s == target {
        return with.clone();
    }
    match s {
        Sym::Add(xs) => add(xs.iter().map(|x| replace_power(x, target, with)).collect()),
        Sym::Mul(xs) => mul(xs.iter().map(|x| replace_power(x, target, with)).collect()),
        Sym::Pow(a, b) => {
            // u^k for u = x^m and x^(mk).
            if let Sym::Pow(tb, te) = target {
                if a == tb {
                    let ratio = div((**b).clone(), (**te).clone());
                    if ratio.as_q().is_some_and(Q::is_int) {
                        return pow(with.clone(), ratio);
                    }
                }
            } else if **a == *target {
                return pow(with.clone(), replace_power(b, target, with));
            }
            pow(
                replace_power(a, target, with),
                replace_power(b, target, with),
            )
        }
        Sym::Fn(f, a) => func(*f, replace_power(a, target, with)),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_terms_combine() {
        let x = var("x");
        let s = add(vec![x.clone(), mul2(n(2), x.clone()), n(3), n(-1)]);
        assert_eq!(s, add(vec![mul2(n(3), x.clone()), n(2)]));
        assert_eq!(mul(vec![x.clone(), x.clone()]), powi(x.clone(), 2));
        assert_eq!(sub(x.clone(), x), n(0));
    }

    #[test]
    fn roots_of_numbers() {
        assert_eq!(sqrt(n(16)), n(4));
        assert_eq!(sqrt(n(12)), mul2(n(2), sqrt(n(3))));
        assert_eq!(mul2(sqrt(n(2)), sqrt(n(2))), n(2));
        assert_eq!(sqrt(frac(1, 4)), frac(1, 2));
        assert_eq!(pow(n(8), frac(1, 3)), n(2));
        assert_eq!(pow(n(-8), frac(1, 3)), n(-2));
    }

    #[test]
    fn exact_trig() {
        assert_eq!(func(Func::Sin, mul2(frac(1, 6), pi())), frac(1, 2));
        assert_eq!(func(Func::Cos, pi()), n(-1));
        assert_eq!(func(Func::Asin, frac(1, 2)), mul2(frac(1, 6), pi()));
        assert_eq!(func(Func::Ln, exp(var("x"))), var("x"));
    }

    #[test]
    fn expansion() {
        let x = var("x");
        let s = expand(&powi(add2(x.clone(), n(1)), 2));
        assert_eq!(s, add(vec![powi(x.clone(), 2), mul2(n(2), x), n(1)]));
    }

    #[test]
    fn decimals_are_exact() {
        assert_eq!(Q::from_f64(0.1), Q::new(1, 10));
        assert_eq!(Q::from_f64(2.5), Q::new(5, 2));
    }
}
