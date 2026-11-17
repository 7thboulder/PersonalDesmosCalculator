//! Proof symbols in rows: number systems (ℕ, ℤ, ℚ, ℝ, …), membership,
//! subsets, set operations, logic (¬ ∧ ∨ ⇒ ⇔) and quantifiers (∀ ∃ ∃! ∄).
//!
//! A row such as `∀n∈ℕ: 1/n > 0` is a statement: it shows true or false.
//! Statements over infinite sets are checked numerically, over the first
//! naturals or integers and over many reals, and say so. A failed ∀ names a
//! counterexample and a true ∃ names its witness.
//!
//! Inside a restriction, as in `y = x^2 \{x ∈ [0, 2]\}`, membership becomes
//! an ordinary condition, so it graphs as any restriction does.

use crate::ast::{Condition, Expr, Relation, ShapeFn};
use crate::builtins::Builtin;
use crate::lexer::SubsetRel;

/// A number system as written: `\mathbb{N}`, `\mathbb{R}^+`, `\mathbb{Z}_{\ge 0}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NumSet {
    /// ℕ = {1, 2, 3, …}.
    Naturals,
    /// ℕ₀ = {0, 1, 2, …}.
    Naturals0,
    Integers,
    /// ℤ⁺ (the same as ℕ), ℤ⁻ and ℤ≥0.
    PosIntegers,
    NegIntegers,
    NonNegIntegers,
    Rationals,
    PosRationals,
    NegRationals,
    Reals,
    PosReals,
    NegReals,
    NonNegReals,
    NonPosReals,
    Complex,
    Primes,
    /// ℝ∖ℚ, also written 𝕀.
    Irrationals,
    /// Sets rows can show but not check: 𝔸, ℍ, 𝕆, 𝔽, ℝⁿ, ℤₙ and others.
    Other(char),
}

impl NumSet {
    /// The set a letter names, with what follows it: `+`, `-`, `0` or `≥0`.
    pub fn from_letter(letter: char, decoration: Option<&str>) -> NumSet {
        use NumSet::*;
        match (letter, decoration) {
            ('N', None | Some("+")) => Naturals,
            ('N', Some("0") | Some("\u{2265}0")) => Naturals0,
            ('Z', None) => Integers,
            ('Z', Some("+") | Some(">0")) => PosIntegers,
            ('Z', Some("-") | Some("<0")) => NegIntegers,
            ('Z', Some("\u{2265}0") | Some("0")) => NonNegIntegers,
            ('Q', None) => Rationals,
            ('Q', Some("+") | Some(">0")) => PosRationals,
            ('Q', Some("-") | Some("<0")) => NegRationals,
            ('R', None) => Reals,
            ('R', Some("+") | Some(">0")) => PosReals,
            ('R', Some("-") | Some("<0")) => NegReals,
            ('R', Some("\u{2265}0") | Some("0")) => NonNegReals,
            ('R', Some("\u{2264}0")) => NonPosReals,
            ('C', None) => Complex,
            ('P', None) => Primes,
            ('I', None) => Irrationals,
            (l, _) => Other(l),
        }
    }

    /// The letter as Unicode, for messages.
    pub fn symbol(self) -> String {
        use NumSet::*;
        match self {
            Naturals => "ℕ".into(),
            Naturals0 => "ℕ₀".into(),
            Integers => "ℤ".into(),
            PosIntegers => "ℤ⁺".into(),
            NegIntegers => "ℤ⁻".into(),
            NonNegIntegers => "ℤ≥0".into(),
            Rationals => "ℚ".into(),
            PosRationals => "ℚ⁺".into(),
            NegRationals => "ℚ⁻".into(),
            Reals => "ℝ".into(),
            PosReals => "ℝ⁺".into(),
            NegReals => "ℝ⁻".into(),
            NonNegReals => "ℝ≥0".into(),
            NonPosReals => "ℝ≤0".into(),
            Complex => "ℂ".into(),
            Primes => "ℙ".into(),
            Irrationals => "𝕀".into(),
            Other(c) => match c {
                'A' => "𝔸".into(),
                'H' => "ℍ".into(),
                'O' => "𝕆".into(),
                'F' => "𝔽".into(),
                other => format!("𝕊({other})"),
            },
        }
    }

    /// Whether its members can be listed one after another (so a
    /// restriction to it plots dots).
    pub fn is_discrete(self) -> bool {
        use NumSet::*;
        matches!(
            self,
            Naturals | Naturals0 | Integers | PosIntegers | NegIntegers | NonNegIntegers | Primes
        )
    }

    /// Whether `x` belongs to the set, as far as a number can tell: 0.5 is
    /// rational and √2 is not, judged by fractions with denominators up
    /// to 10⁴.
    pub fn contains(self, x: f64) -> Result<bool, String> {
        use NumSet::*;
        if x.is_nan() {
            return Ok(false);
        }
        let finite = x.is_finite();
        let int = finite && is_integer(x);
        Ok(match self {
            Naturals | PosIntegers => int && x >= 1.0 - 1e-9,
            Naturals0 | NonNegIntegers => int && x >= -1e-9,
            Integers => int,
            NegIntegers => int && x <= -1.0 + 1e-9,
            Rationals => finite && is_rational(x),
            PosRationals => finite && x > 0.0 && is_rational(x),
            NegRationals => finite && x < 0.0 && is_rational(x),
            Reals | Complex => finite,
            PosReals => finite && x > 0.0,
            NegReals => finite && x < 0.0,
            NonNegReals => finite && x >= 0.0,
            NonPosReals => finite && x <= 0.0,
            Primes => int && is_prime(x.round()),
            Irrationals => finite && !is_rational(x),
            Other(_) => {
                return Err(format!(
                    "Membership in {} can't be checked with numbers",
                    self.symbol()
                ))
            }
        })
    }

    /// Whether every member of `self` is a member of `other`.
    fn within(self, other: NumSet) -> Option<bool> {
        // Each set as a kind of number and a sign.
        #[derive(PartialEq)]
        enum Base {
            Prime,
            Int,
            Rat,
            Irr,
            Real,
            Complex,
        }
        #[derive(PartialEq)]
        enum Sign {
            Any,
            Pos,
            Neg,
            NonNeg,
            NonPos,
        }
        fn parts(s: NumSet) -> Option<(Base, Sign)> {
            use NumSet::*;
            Some(match s {
                Naturals | PosIntegers => (Base::Int, Sign::Pos),
                Naturals0 | NonNegIntegers => (Base::Int, Sign::NonNeg),
                Integers => (Base::Int, Sign::Any),
                NegIntegers => (Base::Int, Sign::Neg),
                Rationals => (Base::Rat, Sign::Any),
                PosRationals => (Base::Rat, Sign::Pos),
                NegRationals => (Base::Rat, Sign::Neg),
                Reals => (Base::Real, Sign::Any),
                PosReals => (Base::Real, Sign::Pos),
                NegReals => (Base::Real, Sign::Neg),
                NonNegReals => (Base::Real, Sign::NonNeg),
                NonPosReals => (Base::Real, Sign::NonPos),
                Complex => (Base::Complex, Sign::Any),
                Primes => (Base::Prime, Sign::Pos),
                Irrationals => (Base::Irr, Sign::Any),
                Other(_) => return None,
            })
        }
        let (a, sa) = parts(self)?;
        let (b, sb) = parts(other)?;
        let rank = |x: &Base| match x {
            Base::Prime => 0,
            Base::Int => 1,
            Base::Rat => 2,
            Base::Irr => 2,
            Base::Real => 3,
            Base::Complex => 4,
        };
        let base_ok = a == b
            || (rank(&a) < rank(&b) && !(a == Base::Irr && b == Base::Rat) && !(b == Base::Irr));
        let sign_ok = sa == sb
            || sb == Sign::Any
            || (sa == Sign::Pos && sb == Sign::NonNeg)
            || (sa == Sign::Neg && sb == Sign::NonPos);
        Some(base_ok && sign_ok)
    }
}

/// Within a billionth of a whole number.
fn is_integer(x: f64) -> bool {
    (x - x.round()).abs() <= 1e-9 * x.abs().max(1.0)
}

/// Whether `x` is a fraction with a denominator up to 10⁴, by continued
/// fractions.
pub fn is_rational(x: f64) -> bool {
    if !x.is_finite() {
        return false;
    }
    let tol = 1e-12 * x.abs().max(1.0);
    let (mut h0, mut h1) = (0.0f64, 1.0f64);
    let (mut k0, mut k1) = (1.0f64, 0.0f64);
    let mut r = x;
    for _ in 0..40 {
        let a = r.floor();
        let h = a * h1 + h0;
        let k = a * k1 + k0;
        if k > 1e4 {
            return false;
        }
        if (x - h / k).abs() <= tol {
            return true;
        }
        let frac = r - a;
        if frac.abs() < 1e-15 {
            return (x - h / k).abs() <= tol;
        }
        r = 1.0 / frac;
        h0 = h1;
        h1 = h;
        k0 = k1;
        k1 = k;
    }
    false
}

pub fn is_prime(n: f64) -> bool {
    if !(2.0..=9.0e15).contains(&n) || n.fract() != 0.0 {
        return false;
    }
    let n = n as u64;
    if n < 4 {
        return true;
    }
    if n % 2 == 0 || n % 3 == 0 {
        return false;
    }
    let mut i = 5u64;
    while i * i <= n {
        if n % i == 0 || n % (i + 2) == 0 {
            return false;
        }
        i += 6;
    }
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetOp {
    Union,
    Inter,
    Diff,
}

/// A set written after ∈, ∀ or ⊂.
#[derive(Clone, Debug, PartialEq)]
pub enum SetExpr {
    Num(NumSet),
    /// `[a, b]`, `(a, b]` and so on; an end can be ±∞.
    Interval {
        lo: Expr,
        hi: Expr,
        lo_closed: bool,
        hi_closed: bool,
    },
    /// A list, such as `A`, `[1, 2, 3]` or `[1...10]`.
    List(Expr),
    Empty,
    Op(SetOp, Box<SetExpr>, Box<SetExpr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quant {
    All,
    Exists,
    ExistsOne,
    NotExists,
}

/// A statement: something true or false.
#[derive(Clone, Debug, PartialEq)]
pub enum Prop {
    /// A comparison, possibly chained: `x < 2`, `0 \le x < 1`, `a = b`.
    Cmp(Condition),
    /// `x ∈ S`, or `x ∉ S` when the flag is set.
    Member(Expr, SetExpr, bool),
    Subset(SetExpr, SetExpr, SubsetRel),
    /// `a ∣ b`, or `a ∤ b` when the flag is set.
    Divides(Expr, Expr, bool),
    Not(Box<Prop>),
    And(Box<Prop>, Box<Prop>),
    Or(Box<Prop>, Box<Prop>),
    Xor(Box<Prop>, Box<Prop>),
    Implies(Box<Prop>, Box<Prop>),
    Iff(Box<Prop>, Box<Prop>),
    /// A quantifier, its variable, the set it ranges over and its body.
    Quant(Quant, String, SetExpr, Box<Prop>),
}

impl SetExpr {
    fn map_exprs(&mut self, f: &mut dyn FnMut(&mut Expr)) {
        match self {
            SetExpr::Num(_) | SetExpr::Empty => {}
            SetExpr::Interval { lo, hi, .. } => {
                f(lo);
                f(hi);
            }
            SetExpr::List(e) => f(e),
            SetExpr::Op(_, a, b) => {
                a.map_exprs(f);
                b.map_exprs(f);
            }
        }
    }

    fn has_quantifier_free_names(&self, f: &mut dyn FnMut(&str)) {
        let mut see = |e: &Expr| e.visit_names(&mut |n, _| f(n));
        match self {
            SetExpr::Num(_) | SetExpr::Empty => {}
            SetExpr::Interval { lo, hi, .. } => {
                see(lo);
                see(hi);
            }
            SetExpr::List(e) => see(e),
            SetExpr::Op(_, a, b) => {
                a.has_quantifier_free_names(f);
                b.has_quantifier_free_names(f);
            }
        }
    }
}

impl Prop {
    /// Applies `f` to every expression in the statement.
    pub fn map_exprs(&mut self, f: &mut dyn FnMut(&mut Expr)) {
        match self {
            Prop::Cmp(c) => c.operands.iter_mut().for_each(f),
            Prop::Member(e, s, _) => {
                f(e);
                s.map_exprs(f);
            }
            Prop::Subset(a, b, _) => {
                a.map_exprs(f);
                b.map_exprs(f);
            }
            Prop::Divides(a, b, _) => {
                f(a);
                f(b);
            }
            Prop::Not(p) => p.map_exprs(f),
            Prop::And(a, b)
            | Prop::Or(a, b)
            | Prop::Xor(a, b)
            | Prop::Implies(a, b)
            | Prop::Iff(a, b) => {
                a.map_exprs(f);
                b.map_exprs(f);
            }
            Prop::Quant(_, _, s, body) => {
                s.map_exprs(f);
                body.map_exprs(f);
            }
        }
    }

    /// Calls `f` on each name the statement reads that no quantifier binds.
    pub fn visit_free_names(&self, f: &mut dyn FnMut(&str)) {
        let mut see = |e: &Expr| e.visit_names(&mut |n, _| f(n));
        match self {
            Prop::Cmp(c) => c.operands.iter().for_each(see),
            Prop::Member(e, s, _) => {
                see(e);
                s.has_quantifier_free_names(f);
            }
            Prop::Subset(a, b, _) => {
                a.has_quantifier_free_names(f);
                b.has_quantifier_free_names(f);
            }
            Prop::Divides(a, b, _) => {
                see(a);
                see(b);
            }
            Prop::Not(p) => p.visit_free_names(f),
            Prop::And(a, b)
            | Prop::Or(a, b)
            | Prop::Xor(a, b)
            | Prop::Implies(a, b)
            | Prop::Iff(a, b) => {
                a.visit_free_names(f);
                b.visit_free_names(f);
            }
            Prop::Quant(_, var, s, body) => {
                s.has_quantifier_free_names(f);
                body.visit_free_names(&mut |n| {
                    if n != var {
                        f(n)
                    }
                });
            }
        }
    }

    fn depth(&self) -> usize {
        match self {
            Prop::Cmp(_) | Prop::Member(..) | Prop::Subset(..) | Prop::Divides(..) => 0,
            Prop::Not(p) => p.depth(),
            Prop::And(a, b)
            | Prop::Or(a, b)
            | Prop::Xor(a, b)
            | Prop::Implies(a, b)
            | Prop::Iff(a, b) => a.depth().max(b.depth()),
            Prop::Quant(_, _, _, body) => 1 + body.depth(),
        }
    }
}

// ---- Restrictions ----------------------------------------------------------

fn num(v: f64) -> Expr {
    Expr::Num(v)
}

/// 1 where `elem` is in `set` and 0 where it isn't, as an expression the
/// graph can work out anywhere.
pub fn indicator(elem: &Expr, set: &SetExpr) -> Result<Expr, String> {
    Ok(match set {
        SetExpr::Num(NumSet::Other(_)) => {
            return Err(format!(
                "Restricting to {} isn't something a graph can do",
                match set {
                    SetExpr::Num(s) => s.symbol(),
                    _ => unreachable!(),
                }
            ))
        }
        SetExpr::Num(s) => Expr::Call(Builtin::InSet(*s), vec![elem.clone()]),
        SetExpr::Interval { .. } => Expr::Piecewise(
            vec![(
                interval_condition(elem, set).expect("an interval"),
                num(1.0),
            )],
            Some(Box::new(num(0.0))),
        ),
        SetExpr::List(list) => Expr::Shape(ShapeFn::Member, vec![elem.clone(), list.clone()]),
        SetExpr::Empty => num(0.0),
        SetExpr::Op(op, a, b) => {
            let (a, b) = (indicator(elem, a)?, indicator(elem, b)?);
            let mul = |a: Expr, b: Expr| Expr::bin(Expr::Mul, a, b);
            match op {
                SetOp::Union => Expr::bin(
                    Expr::Sub,
                    Expr::bin(Expr::Add, a.clone(), b.clone()),
                    mul(a, b),
                ),
                SetOp::Inter => mul(a, b),
                SetOp::Diff => mul(a, Expr::bin(Expr::Sub, num(1.0), b)),
            }
        }
    })
}

/// `lo ≤ x ≤ hi` for `x ∈ [lo, hi]`, with `<` at open ends, leaving out an
/// infinite end.
fn interval_condition(elem: &Expr, set: &SetExpr) -> Option<Condition> {
    let SetExpr::Interval {
        lo,
        hi,
        lo_closed,
        hi_closed,
    } = set
    else {
        return None;
    };
    let infinite = |e: &Expr, sign: f64| match e {
        Expr::Var(n) if n == "infty" => sign > 0.0,
        Expr::Neg(inner) => matches!(&**inner, Expr::Var(n) if n == "infty") && sign < 0.0,
        _ => false,
    };
    let rel = |closed: bool| if closed { Relation::Le } else { Relation::Lt };
    let mut operands = Vec::new();
    let mut relations = Vec::new();
    if !infinite(lo, -1.0) {
        operands.push(lo.clone());
        relations.push(rel(*lo_closed));
    }
    operands.push(elem.clone());
    if !infinite(hi, 1.0) {
        operands.push(hi.clone());
        relations.push(rel(*hi_closed));
    }
    if relations.is_empty() {
        // (−∞, ∞): everything; x = x holds wherever x is defined.
        operands.push(elem.clone());
        relations.push(Relation::Eq);
    }
    Some(Condition {
        operands,
        relations,
    })
}

/// The condition a restriction like `\{x ∈ [0, 2]\}` stands for.
pub fn member_condition(elem: &Expr, set: &SetExpr, negated: bool) -> Result<Condition, String> {
    if !negated {
        if let Some(c) = interval_condition(elem, set) {
            return Ok(c);
        }
    }
    Ok(Condition {
        operands: vec![indicator(elem, set)?, num(if negated { 0.0 } else { 1.0 })],
        relations: vec![Relation::Eq],
    })
}

/// A statement inside a restriction, as a condition: `\{x > 0 ∧ x ∈ ℤ\}`.
pub fn restriction(prop: &Prop) -> Result<Condition, String> {
    Ok(match prop {
        Prop::Cmp(c) => c.clone(),
        Prop::Member(e, s, neg) => member_condition(e, s, *neg)?,
        _ => Condition {
            operands: vec![prop_indicator(prop)?, num(1.0)],
            relations: vec![Relation::Eq],
        },
    })
}

/// 1 where the statement holds and 0 where it doesn't.
fn prop_indicator(prop: &Prop) -> Result<Expr, String> {
    let one = || num(1.0);
    let sub = |a: Expr, b: Expr| Expr::bin(Expr::Sub, a, b);
    let mul = |a: Expr, b: Expr| Expr::bin(Expr::Mul, a, b);
    Ok(match prop {
        Prop::Cmp(c) => Expr::Piecewise(vec![(c.clone(), one())], Some(Box::new(num(0.0)))),
        Prop::Member(e, s, neg) => {
            let i = indicator(e, s)?;
            if *neg {
                sub(one(), i)
            } else {
                i
            }
        }
        Prop::Divides(a, b, neg) => {
            let m = Expr::Call(Builtin::Mod, vec![b.clone(), a.clone()]);
            let zero = Condition {
                operands: vec![m, num(0.0)],
                relations: vec![Relation::Eq],
            };
            let i = Expr::Piecewise(vec![(zero, one())], Some(Box::new(num(0.0))));
            if *neg {
                sub(one(), i)
            } else {
                i
            }
        }
        Prop::Not(p) => sub(one(), prop_indicator(p)?),
        Prop::And(a, b) => mul(prop_indicator(a)?, prop_indicator(b)?),
        Prop::Or(a, b) => {
            let (a, b) = (prop_indicator(a)?, prop_indicator(b)?);
            sub(Expr::bin(Expr::Add, a.clone(), b.clone()), mul(a, b))
        }
        Prop::Xor(a, b) => {
            let d = sub(prop_indicator(a)?, prop_indicator(b)?);
            mul(d.clone(), d)
        }
        Prop::Implies(a, b) => sub(
            one(),
            mul(prop_indicator(a)?, sub(one(), prop_indicator(b)?)),
        ),
        Prop::Iff(a, b) => {
            let d = sub(prop_indicator(a)?, prop_indicator(b)?);
            sub(one(), mul(d.clone(), d))
        }
        Prop::Subset(..) => return Err("A subset can't restrict a graph".into()),
        Prop::Quant(..) => {
            return Err("∀ and ∃ make a statement of their own row, not a restriction".into())
        }
    })
}

// ---- Checking statements ---------------------------------------------------

/// What went wrong working a statement out: a message, and names that are
/// not defined (offered as sliders).
#[derive(Debug, Default)]
pub struct Failure {
    pub message: String,
    pub missing: Vec<String>,
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Failure {
            message,
            missing: Vec::new(),
        }
    }
}

impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        Failure::from(message.to_owned())
    }
}

/// A number program of the quantified variables (at most three).
pub type Program = Box<dyn Fn(&[f64; 3]) -> f64>;

/// What the graph provides to check statements.
pub trait Eval {
    /// `e` as a program of the variables `bound` (in order), which must be
    /// a number wherever they are.
    fn program(&mut self, e: &Expr, bound: &[String]) -> Result<Program, Failure>;
    /// The numbers in a list (or a single number) that reads none of `bound`.
    fn numbers(&mut self, e: &Expr, bound: &[String]) -> Result<Vec<f64>, Failure>;
}

/// The answer for a statement row, as LaTeX shown after `=`.
#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    pub holds: Option<bool>,
    pub latex: String,
}

enum CSet {
    Num(NumSet),
    Interval {
        lo: Program,
        hi: Program,
        lo_closed: bool,
        hi_closed: bool,
    },
    List(Vec<f64>),
    Empty,
    Op(SetOp, Box<CSet>, Box<CSet>),
}

enum CProp {
    Cmp(Vec<Program>, Vec<Relation>),
    Member(Program, CSet, bool),
    Subset(bool),
    Divides(Program, Program, bool),
    Not(Box<CProp>),
    And(Box<CProp>, Box<CProp>),
    Or(Box<CProp>, Box<CProp>),
    Xor(Box<CProp>, Box<CProp>),
    Implies(Box<CProp>, Box<CProp>),
    Iff(Box<CProp>, Box<CProp>),
    Quant {
        kind: Quant,
        var: String,
        slot: usize,
        set: CSet,
        body: Box<CProp>,
    },
}

/// How many values a quantifier tries, by how many quantifiers are nested:
/// discrete sets, then continuous ones.
fn budget(levels: usize) -> (usize, usize) {
    match levels {
        0 | 1 => (10_000, 20_000),
        2 => (400, 600),
        _ => (40, 60),
    }
}

struct Ctx<'a> {
    ev: &'a mut dyn Eval,
    bound: Vec<String>,
}

impl Ctx<'_> {
    fn program(&mut self, e: &Expr) -> Result<Program, Failure> {
        self.ev.program(e, &self.bound)
    }

    fn set(&mut self, s: &SetExpr) -> Result<CSet, Failure> {
        Ok(match s {
            SetExpr::Num(n) => CSet::Num(*n),
            SetExpr::Interval {
                lo,
                hi,
                lo_closed,
                hi_closed,
            } => CSet::Interval {
                lo: self.program(lo)?,
                hi: self.program(hi)?,
                lo_closed: *lo_closed,
                hi_closed: *hi_closed,
            },
            SetExpr::List(e) => CSet::List(self.ev.numbers(e, &self.bound)?),
            SetExpr::Empty => CSet::Empty,
            SetExpr::Op(op, a, b) => CSet::Op(*op, Box::new(self.set(a)?), Box::new(self.set(b)?)),
        })
    }

    fn prop(&mut self, p: &Prop) -> Result<CProp, Failure> {
        let two =
            |me: &mut Self, a: &Prop, b: &Prop| -> Result<(Box<CProp>, Box<CProp>), Failure> {
                Ok((Box::new(me.prop(a)?), Box::new(me.prop(b)?)))
            };
        Ok(match p {
            Prop::Cmp(c) => CProp::Cmp(
                c.operands
                    .iter()
                    .map(|e| self.program(e))
                    .collect::<Result<_, _>>()?,
                c.relations.clone(),
            ),
            Prop::Member(e, s, neg) => CProp::Member(self.program(e)?, self.set(s)?, *neg),
            Prop::Subset(a, b, rel) => CProp::Subset(self.subset(a, b, *rel)?),
            Prop::Divides(a, b, neg) => CProp::Divides(self.program(a)?, self.program(b)?, *neg),
            Prop::Not(a) => CProp::Not(Box::new(self.prop(a)?)),
            Prop::And(a, b) => {
                let (a, b) = two(self, a, b)?;
                CProp::And(a, b)
            }
            Prop::Or(a, b) => {
                let (a, b) = two(self, a, b)?;
                CProp::Or(a, b)
            }
            Prop::Xor(a, b) => {
                let (a, b) = two(self, a, b)?;
                CProp::Xor(a, b)
            }
            Prop::Implies(a, b) => {
                let (a, b) = two(self, a, b)?;
                CProp::Implies(a, b)
            }
            Prop::Iff(a, b) => {
                let (a, b) = two(self, a, b)?;
                CProp::Iff(a, b)
            }
            Prop::Quant(kind, var, set, body) => {
                if self.bound.len() >= 3 {
                    return Err("At most three nested ∀ or ∃ can be checked".into());
                }
                if let SetExpr::Num(NumSet::Complex) = set {
                    return Err("Statements over ℂ can't be checked with real numbers".into());
                }
                let set = self.set(set)?;
                let slot = self.bound.len();
                self.bound.push(var.clone());
                let body = self.prop(body);
                self.bound.pop();
                CProp::Quant {
                    kind: *kind,
                    var: var.clone(),
                    slot,
                    set,
                    body: Box::new(body?),
                }
            }
        })
    }

    /// Whether `a ⊆ b` (as `rel` asks), for lists, intervals with constant
    /// ends and number systems.
    fn subset(&mut self, a: &SetExpr, b: &SetExpr, rel: SubsetRel) -> Result<bool, Failure> {
        let (a, b) = if rel.superset { (b, a) } else { (a, b) };
        let within = self.within(a, b)?;
        if rel.proper && within {
            // A proper subset leaves something out.
            let equal = self.within(b, a)?;
            return Ok(!equal != rel.negated);
        }
        Ok(within != rel.negated)
    }

    fn within(&mut self, a: &SetExpr, b: &SetExpr) -> Result<bool, Failure> {
        if let (SetExpr::Num(x), SetExpr::Num(y)) = (a, b) {
            return x.within(*y).ok_or_else(|| {
                format!("{} can't be compared with {}", x.symbol(), y.symbol()).into()
            });
        }
        let none = [f64::NAN; 3];
        let (ca, cb) = (self.set(a)?, self.set(b)?);
        if let Some(items) = finite_members(&ca)? {
            for x in items {
                if !member(&cb, x, &none)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        let constant = |p: &Program| p(&none);
        Ok(match (&ca, &cb) {
            // A number system is unbounded, so it fits in no interval
            // (ℝ⁺ in (0, ∞) aside, which is rare enough to leave).
            (CSet::Num(_), CSet::Interval { .. }) => false,
            (CSet::Interval { .. }, CSet::Num(y)) => matches!(y, NumSet::Reals | NumSet::Complex),
            (
                CSet::Interval {
                    lo,
                    hi,
                    lo_closed,
                    hi_closed,
                },
                CSet::Interval {
                    lo: lo2,
                    hi: hi2,
                    lo_closed: c1,
                    hi_closed: c2,
                },
            ) => {
                let (lo, hi, lo2, hi2) = (constant(lo), constant(hi), constant(lo2), constant(hi2));
                if [lo, hi, lo2, hi2].iter().any(|v| v.is_nan()) {
                    return Err("An interval's ends must be numbers".into());
                }
                (lo > lo2 || (lo == lo2 && (*c1 || !lo_closed)))
                    && (hi < hi2 || (hi == hi2 && (*c2 || !hi_closed)))
            }
            _ => return Err("This subset can't be checked".into()),
        })
    }
}

/// Every member of a set, when there are finitely many.
fn finite_members(set: &CSet) -> Result<Option<Vec<f64>>, Failure> {
    let none = [f64::NAN; 3];
    Ok(match set {
        CSet::List(items) => Some(items.clone()),
        CSet::Empty => Some(Vec::new()),
        CSet::Op(op, a, b) => match (op, finite_members(a)?, finite_members(b)?) {
            (SetOp::Union, Some(mut x), Some(y)) => {
                x.extend(y);
                Some(x)
            }
            (SetOp::Inter | SetOp::Diff, Some(x), _) => {
                let keep = *op == SetOp::Inter;
                let mut out = Vec::new();
                for v in x {
                    if member(b, v, &none)? == keep {
                        out.push(v);
                    }
                }
                Some(out)
            }
            (SetOp::Inter, None, Some(y)) => {
                let mut out = Vec::new();
                for v in y {
                    if member(a, v, &none)? {
                        out.push(v);
                    }
                }
                Some(out)
            }
            _ => None,
        },
        _ => None,
    })
}

const TRUE: Option<bool> = Some(true);

/// Equal up to rounding: a billionth of their size, or 10⁻¹³ near 0
/// (sin π is 1.2·10⁻¹⁶, not 0).
fn close(a: f64, b: f64) -> bool {
    a == b || (a - b).abs() <= 1e-13 + 1e-9 * a.abs().max(b.abs())
}

fn compare(a: f64, rel: Relation, b: f64) -> Option<bool> {
    if a.is_nan() || b.is_nan() {
        return None;
    }
    Some(match rel {
        Relation::Eq => close(a, b),
        Relation::Lt => a < b,
        Relation::Gt => a > b,
        Relation::Le => a <= b || close(a, b),
        Relation::Ge => a >= b || close(a, b),
    })
}

fn member(set: &CSet, x: f64, args: &[f64; 3]) -> Result<bool, Failure> {
    Ok(match set {
        CSet::Num(n) => n.contains(x)?,
        CSet::Interval {
            lo,
            hi,
            lo_closed,
            hi_closed,
        } => {
            let (lo, hi) = (lo(args), hi(args));
            if x.is_nan() || lo.is_nan() || hi.is_nan() {
                false
            } else {
                (if *lo_closed { x >= lo } else { x > lo })
                    && (if *hi_closed { x <= hi } else { x < hi })
            }
        }
        CSet::List(items) => items.iter().any(|v| close(*v, x)),
        CSet::Empty => false,
        CSet::Op(op, a, b) => {
            let (a, b) = (member(a, x, args)?, member(b, x, args)?);
            match op {
                SetOp::Union => a || b,
                SetOp::Inter => a && b,
                SetOp::Diff => a && !b,
            }
        }
    })
}

/// The values a quantifier tries, and how to describe them when the check
/// is not exhaustive (None when every member was tried).
/// Values worth trying for an inner quantifier, from the outer ones: a δ
/// for an ε is often ε/2, ε² or √ε, and an N is often 1/ε.
fn scaled(args: &[f64; 3], slot: usize) -> Vec<f64> {
    let mut out = Vec::new();
    for &a in &args[..slot] {
        if !a.is_finite() || a == 0.0 {
            continue;
        }
        for v in [
            a,
            a / 2.0,
            a / 3.0,
            a / 4.0,
            a / 10.0,
            a / 100.0,
            a * 1e-3,
            a * 1e-6,
            a * a,
            a.abs().sqrt(),
            1.0 / a,
            2.0 / a,
            10.0 / a,
            1.0 / (a * a),
        ] {
            if v.is_finite() {
                out.push(v);
                out.push(-v);
                out.push(v.ceil());
                out.push(v.floor() + 1.0);
                out.push(-v.ceil());
            }
        }
    }
    out
}

fn samples(
    set: &CSet,
    args: &[f64; 3],
    var: &str,
    levels: usize,
    slot: usize,
) -> Result<(Vec<f64>, Option<String>), Failure> {
    let (mut xs, note) = base_samples(set, args, var, levels, slot)?;
    // Inner quantifiers also try values built from the outer ones, and
    // naturals and integers a few large values.
    let finite = matches!(set, CSet::List(_) | CSet::Empty);
    if !finite {
        let mut extra = scaled(args, slot);
        if matches!(set, CSet::Num(n) if n.is_discrete()) {
            for p in 4..=15 {
                for m in [1.0, 2.0, 5.0] {
                    extra.push(m * 10f64.powi(p));
                    extra.push(-m * 10f64.powi(p));
                }
            }
        }
        for x in extra {
            if member(set, x, args)? {
                xs.push(x);
            }
        }
    }
    Ok((xs, note))
}

fn base_samples(
    set: &CSet,
    args: &[f64; 3],
    var: &str,
    levels: usize,
    slot: usize,
) -> Result<(Vec<f64>, Option<String>), Failure> {
    let (discrete, continuous) = budget(levels);
    let v = crate::format::name_latex(var);
    Ok(match set {
        CSet::Num(n) => {
            use NumSet::*;
            let n = *n;
            let d = discrete as i64;
            match n {
                Naturals | PosIntegers => (
                    (1..=d).map(|k| k as f64).collect(),
                    Some(format!("1\\le {v}\\le {d}")),
                ),
                Naturals0 | NonNegIntegers => (
                    (0..d).map(|k| k as f64).collect(),
                    Some(format!("0\\le {v}\\le {}", d - 1)),
                ),
                NegIntegers => (
                    (1..=d).map(|k| -(k as f64)).collect(),
                    Some(format!("-{d}\\le {v}\\le -1")),
                ),
                Integers => {
                    let half = d / 2;
                    let mut out = vec![0.0];
                    for k in 1..=half {
                        out.push(k as f64);
                        out.push(-(k as f64));
                    }
                    (out, Some(format!("\\left|{v}\\right|\\le {half}")))
                }
                Primes => {
                    let mut out = Vec::new();
                    let mut k = 2u64;
                    while out.len() < discrete {
                        if is_prime(k as f64) {
                            out.push(k as f64);
                        }
                        k += 1;
                    }
                    let last = *out.last().unwrap_or(&2.0);
                    (out, Some(format!("{v}\\le {last}")))
                }
                Rationals | PosRationals | NegRationals => {
                    let mut out = Vec::new();
                    let max_q = ((continuous as f64).sqrt() / 4.0).max(3.0) as i64;
                    for q in 1..=max_q {
                        for p in -(4 * q)..=(4 * q) {
                            if gcd(p.unsigned_abs(), q as u64) == 1 {
                                let x = p as f64 / q as f64;
                                if n.contains(x)? {
                                    out.push(x);
                                }
                            }
                        }
                    }
                    for k in 1..=60 {
                        for x in [
                            k as f64 * 10.0,
                            -(k as f64) * 10.0,
                            1.0 / (k as f64 * 10.0),
                            -1.0 / (k as f64 * 10.0),
                        ] {
                            if n.contains(x)? {
                                out.push(x);
                            }
                        }
                    }
                    (out, Some("\\text{fractions}".into()))
                }
                Reals | PosReals | NegReals | NonNegReals | NonPosReals | Irrationals => {
                    let mut out = Vec::new();
                    for x in real_samples(continuous) {
                        if n.contains(x)? {
                            out.push(x);
                        }
                    }
                    (out, Some("\\text{numerically}".into()))
                }
                Complex | Other(_) => {
                    return Err(format!(
                        "Statements over {} can't be checked with numbers",
                        n.symbol()
                    )
                    .into())
                }
            }
        }
        CSet::Interval {
            lo,
            hi,
            lo_closed,
            hi_closed,
        } => {
            let (lo, hi) = (lo(args), hi(args));
            if lo.is_nan() || hi.is_nan() {
                return Err("An interval's ends must be numbers".into());
            }
            let mut out = Vec::new();
            if lo.is_finite() && hi.is_finite() {
                let n = continuous.max(2);
                for k in 0..=n {
                    out.push(lo + (hi - lo) * k as f64 / n as f64);
                }
                // Close to each end, where ε-δ arguments live.
                for p in 1..=12 {
                    let t = 10f64.powi(-p);
                    out.push(lo + (hi - lo) * t);
                    out.push(hi - (hi - lo) * t);
                }
            } else {
                for x in real_samples(continuous) {
                    out.push(x);
                }
                if lo.is_finite() {
                    for p in -12..=6 {
                        out.push(lo + 10f64.powi(p));
                        out.push(lo + 3.0 * 10f64.powi(p));
                    }
                }
                if hi.is_finite() {
                    for p in -12..=6 {
                        out.push(hi - 10f64.powi(p));
                        out.push(hi - 3.0 * 10f64.powi(p));
                    }
                }
            }
            let probe = CSet::Interval {
                lo: Box::new(move |_| lo),
                hi: Box::new(move |_| hi),
                lo_closed: *lo_closed,
                hi_closed: *hi_closed,
            };
            out.retain(|x| member(&probe, *x, args).unwrap_or(false));
            (out, Some("\\text{numerically}".into()))
        }
        CSet::List(items) => (items.clone(), None),
        CSet::Empty => (Vec::new(), None),
        CSet::Op(op, a, b) => {
            let (mut xs, note_a) = base_samples(a, args, var, levels, slot)?;
            let note = match op {
                SetOp::Union => {
                    let (ys, note_b) = base_samples(b, args, var, levels, slot)?;
                    xs.extend(ys);
                    note_a.or(note_b)
                }
                SetOp::Inter | SetOp::Diff => {
                    let keep_in = *op == SetOp::Inter;
                    let mut kept = Vec::new();
                    for x in xs {
                        if member(b, x, args)? == keep_in {
                            kept.push(x);
                        }
                    }
                    xs = kept;
                    // ℝ∖ℚ: a grid is all fractions, so add irrational points.
                    if !keep_in && xs.len() < 10 {
                        for x in real_samples(continuous_of(levels)) {
                            let x = x + std::f64::consts::SQRT_2 * 1e-3;
                            if member(a, x, args)? && !member(b, x, args)? {
                                xs.push(x);
                            }
                        }
                    }
                    note_a
                }
            };
            xs.sort_by(|a, b| a.abs().total_cmp(&b.abs()));
            xs.dedup_by(|a, b| a == b);
            (xs, note)
        }
    })
}

fn continuous_of(levels: usize) -> usize {
    budget(levels).1
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Reals to try: a fine grid near 0, a coarser one further out, powers of
/// ten up to a million and down to a billionth, and some irrationals.
fn real_samples(n: usize) -> Vec<f64> {
    let mut out = vec![0.0];
    let fine = n / 2;
    for k in 1..=fine {
        let x = 10.0 * k as f64 / fine as f64;
        out.push(x);
        out.push(-x);
    }
    let coarse = n / 8;
    for k in 1..=coarse {
        let x = 10.0 + 990.0 * k as f64 / coarse as f64;
        out.push(x);
        out.push(-x);
    }
    for p in -9..=6 {
        for m in [1.0, 2.0, 5.0] {
            let x = m * 10f64.powi(p);
            out.push(x);
            out.push(-x);
        }
    }
    for c in [
        std::f64::consts::PI,
        std::f64::consts::E,
        std::f64::consts::SQRT_2,
        0.5 * std::f64::consts::SQRT_2,
        3f64.sqrt(),
    ] {
        out.push(c);
        out.push(-c);
    }
    out
}

/// What checking found: whether it holds, and the value of the outermost
/// variable that decided it (a counterexample or a witness).
struct Found {
    holds: Option<bool>,
    at: Option<f64>,
    note: Option<String>,
    /// For ∃!: a second witness.
    second: Option<f64>,
}

fn not(a: Option<bool>) -> Option<bool> {
    a.map(|a| !a)
}

fn and(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(false), _) | (_, Some(false)) => Some(false),
        (Some(true), Some(true)) => TRUE,
        _ => None,
    }
}

fn or(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(true), _) | (_, Some(true)) => TRUE,
        (Some(false), Some(false)) => Some(false),
        _ => None,
    }
}

fn truth(p: &CProp, args: &mut [f64; 3], levels: usize) -> Result<Option<bool>, Failure> {
    Ok(eval(p, args, levels)?.holds)
}

fn eval(p: &CProp, args: &mut [f64; 3], levels: usize) -> Result<Found, Failure> {
    let plain = |holds| Found {
        holds,
        at: None,
        note: None,
        second: None,
    };
    Ok(match p {
        CProp::Cmp(operands, relations) => {
            let values: Vec<f64> = operands.iter().map(|f| f(args)).collect();
            let mut holds = TRUE;
            for (k, rel) in relations.iter().enumerate() {
                holds = and(holds, compare(values[k], *rel, values[k + 1]));
            }
            plain(holds)
        }
        CProp::Member(e, set, neg) => {
            let x = e(args);
            if x.is_nan() {
                plain(None)
            } else {
                plain(Some(member(set, x, args)? != *neg))
            }
        }
        CProp::Subset(holds) => plain(Some(*holds)),
        CProp::Divides(a, b, neg) => {
            let (a, b) = (a(args), b(args));
            if a.is_nan() || b.is_nan() {
                plain(None)
            } else if !is_integer(a) || !is_integer(b) {
                return Err("∣ (divides) is for whole numbers".into());
            } else {
                let (a, b) = (a.round(), b.round());
                let divides = if a == 0.0 { b == 0.0 } else { (b % a) == 0.0 };
                plain(Some(divides != *neg))
            }
        }
        CProp::Not(a) => plain(not(truth(a, args, levels)?)),
        CProp::And(a, b) => {
            let a = truth(a, args, levels)?;
            if a == Some(false) {
                plain(a)
            } else {
                plain(and(a, truth(b, args, levels)?))
            }
        }
        CProp::Or(a, b) => {
            let a = truth(a, args, levels)?;
            if a == TRUE {
                plain(a)
            } else {
                plain(or(a, truth(b, args, levels)?))
            }
        }
        CProp::Xor(a, b) => {
            let (a, b) = (truth(a, args, levels)?, truth(b, args, levels)?);
            plain(a.zip(b).map(|(a, b)| a != b))
        }
        CProp::Implies(a, b) => {
            let a = truth(a, args, levels)?;
            if a == Some(false) {
                plain(TRUE)
            } else {
                plain(or(not(a), truth(b, args, levels)?))
            }
        }
        CProp::Iff(a, b) => {
            let (a, b) = (truth(a, args, levels)?, truth(b, args, levels)?);
            plain(a.zip(b).map(|(a, b)| a == b))
        }
        CProp::Quant {
            kind,
            var,
            slot,
            set,
            body,
        } => {
            let (xs, note) = samples(set, args, var, levels, *slot)?;
            let saved = args[*slot];
            let mut result = Found {
                holds: match kind {
                    Quant::All | Quant::NotExists => TRUE,
                    Quant::Exists | Quant::ExistsOne => Some(false),
                },
                at: None,
                note,
                second: None,
            };
            let mut witnesses: Vec<f64> = Vec::new();
            for &x in &xs {
                args[*slot] = x;
                let t = truth(body, args, levels)?;
                match kind {
                    Quant::All => {
                        // Far out, undefined is usually the arithmetic
                        // running out (a sum too long to add up), not a
                        // counterexample.
                        if t.is_none() && x.abs() > 1e4 {
                            continue;
                        }
                        if t != TRUE {
                            result.holds = Some(false);
                            result.at = Some(x);
                            break;
                        }
                    }
                    Quant::Exists | Quant::NotExists => {
                        if t == TRUE {
                            result.holds = Some(*kind == Quant::Exists);
                            result.at = Some(x);
                            break;
                        }
                    }
                    Quant::ExistsOne => {
                        if t == TRUE && !witnesses.iter().any(|w| close(*w, x)) {
                            witnesses.push(x);
                            if witnesses.len() > 1 {
                                break;
                            }
                        }
                    }
                }
            }
            // Over a continuous set, an equation is rarely hit exactly on a
            // grid: look for where its two sides cross, and try there.
            let continuous = !matches!(set, CSet::List(_) | CSet::Empty)
                && !matches!(set, CSet::Num(n) if n.is_discrete());
            let searching = match kind {
                Quant::Exists | Quant::NotExists => result.at.is_none(),
                Quant::ExistsOne => witnesses.len() < 2,
                Quant::All => false,
            };
            if continuous && searching {
                if let Some(diff) = equation_of(body) {
                    let mut roots = Vec::new();
                    let mut sorted = xs.clone();
                    sorted.sort_by(|a, b| a.total_cmp(b));
                    let mut prev: Option<(f64, f64)> = None;
                    for &x in &sorted {
                        args[*slot] = x;
                        let d = diff(args);
                        if let Some((px, pd)) = prev {
                            if d.is_finite()
                                && pd.is_finite()
                                && (pd < 0.0) != (d < 0.0)
                                && d != 0.0
                            {
                                if let Some(r) = bisect(&diff, args, *slot, px, x, pd) {
                                    roots.push(r);
                                }
                            }
                        }
                        if d.is_finite() {
                            prev = Some((x, d));
                        }
                    }
                    for r in roots {
                        if !member(set, r, args)? {
                            continue;
                        }
                        args[*slot] = r;
                        if truth(body, args, levels)? == TRUE {
                            if *kind == Quant::ExistsOne {
                                if !witnesses.iter().any(|w| close(*w, r)) {
                                    witnesses.push(r);
                                }
                            } else {
                                result.holds = Some(*kind == Quant::Exists);
                                result.at = Some(r);
                                break;
                            }
                        }
                    }
                }
            }
            if *kind == Quant::ExistsOne {
                result.holds = Some(witnesses.len() == 1);
                result.at = witnesses.first().copied();
                result.second = witnesses.get(1).copied();
            }
            args[*slot] = saved;
            result
        }
    })
}

/// For a body that is (or starts with) an equation `a = b`, the program
/// a − b, to search for where it is 0.
fn equation_of(p: &CProp) -> Option<Box<dyn Fn(&[f64; 3]) -> f64 + '_>> {
    match p {
        CProp::Cmp(ops, rels) if rels.len() == 1 && rels[0] == Relation::Eq => {
            let (a, b) = (&ops[0], &ops[1]);
            Some(Box::new(move |args: &[f64; 3]| a(args) - b(args)))
        }
        CProp::And(a, b) => equation_of(a).or_else(|| equation_of(b)),
        _ => None,
    }
}

fn bisect(
    f: &dyn Fn(&[f64; 3]) -> f64,
    args: &mut [f64; 3],
    slot: usize,
    mut a: f64,
    mut b: f64,
    fa: f64,
) -> Option<f64> {
    let neg = fa < 0.0;
    for _ in 0..200 {
        let m = 0.5 * (a + b);
        if m == a || m == b {
            break;
        }
        args[slot] = m;
        let fm = f(args);
        if !fm.is_finite() {
            return None;
        }
        if (fm < 0.0) == neg {
            a = m;
        } else {
            b = m;
        }
    }
    let r = 0.5 * (a + b);
    // Snap to a whole number or a simple fraction when it is one.
    let snapped = [r.round(), (r * 2.0).round() / 2.0, (r * 3.0).round() / 3.0]
        .into_iter()
        .find(|s| (s - r).abs() < 1e-12 * r.abs().max(1.0))
        .unwrap_or(r);
    Some(snapped)
}

/// Checks a statement, numerically where it ranges over infinite sets.
pub fn check(prop: &Prop, ev: &mut dyn Eval) -> Result<Verdict, Failure> {
    let levels = prop.depth();
    let mut ctx = Ctx {
        ev,
        bound: Vec::new(),
    };
    let compiled = ctx.prop(prop)?;
    let mut args = [f64::NAN; 3];
    let found = eval(&compiled, &mut args, levels.max(1))?;
    let word = |b: bool| if b { "\\text{true}" } else { "\\text{false}" };
    let latex = match (&compiled, found.holds) {
        (_, None) => "\\text{undefined}".to_owned(),
        (CProp::Quant { kind, var, .. }, Some(holds)) => {
            let v = crate::format::name_latex(var);
            let at = |x: f64| format!("{v}={}", crate::format::value_latex(x));
            let checked = |note: &Option<String>| match note {
                Some(n) if n.starts_with("\\text") => format!(
                    "\\quad\\left(\\text{{checked {}}}\\right)",
                    &n[6..n.len() - 1]
                ),
                Some(n) => format!("\\quad\\left(\\text{{checked for }}{n}\\right)"),
                None => String::new(),
            };
            match (kind, holds) {
                (Quant::All, true) | (Quant::NotExists, true) => {
                    format!("{}{}", word(true), checked(&found.note))
                }
                (Quant::All, false) => match found.at {
                    Some(x) => format!("\\text{{false at }}{}", at(x)),
                    None => word(false).into(),
                },
                (Quant::Exists, true) | (Quant::ExistsOne, true) => match found.at {
                    Some(x) => format!("\\text{{true: }}{}", at(x)),
                    None => word(true).into(),
                },
                (Quant::NotExists, false) => match found.at {
                    Some(x) => format!("\\text{{false: }}{}", at(x)),
                    None => word(false).into(),
                },
                (Quant::Exists, false) => {
                    format!(
                        "{}{}",
                        word(false),
                        checked(&found.note).replace("checked", "none found")
                    )
                }
                (Quant::ExistsOne, false) => match (found.at, found.second) {
                    (Some(a), Some(b)) => {
                        format!("\\text{{false: }}{}\\text{{ and }}{}", at(a), at(b))
                    }
                    _ => format!(
                        "{}{}",
                        word(false),
                        checked(&found.note).replace("checked", "none found")
                    ),
                },
            }
        }
        (_, Some(holds)) => word(holds).to_owned(),
    };
    Ok(Verdict {
        holds: found.holds,
        latex,
    })
}

/// Rows with these tokens outside braces are statements.
pub fn is_statement_token(t: &crate::lexer::Tok) -> bool {
    use crate::lexer::Tok::*;
    matches!(
        t,
        In | NotIn
            | Subset(_)
            | ForAll
            | Exists
            | ExistsOne
            | NExists
            | Not
            | And
            | Or
            | Xor
            | Implies
            | ImpliedBy
            | Iff
            | Divides
            | NotDivides
    )
}

// ---- Dots for discrete restrictions ----------------------------------------

/// The set a restriction condition limits `var` to, when it is discrete:
/// `\{x ∈ ℤ\}` or `\{x ∈ A\}` for a list A.
enum Discrete {
    Num(NumSet),
    List(Expr),
}

fn discrete_restriction(c: &Condition, var: &str) -> Option<Discrete> {
    if c.relations != [Relation::Eq] || c.operands.get(1) != Some(&Expr::Num(1.0)) {
        return None;
    }
    let is_var = |e: &Expr| matches!(e, Expr::Var(v) if v == var);
    match &c.operands[0] {
        Expr::Call(Builtin::InSet(s), args)
            if s.is_discrete() && args.len() == 1 && is_var(&args[0]) =>
        {
            Some(Discrete::Num(*s))
        }
        Expr::Shape(ShapeFn::Member, args) if args.len() == 2 && is_var(&args[0]) => {
            Some(Discrete::List(args[1].clone()))
        }
        _ => None,
    }
}

/// Every restriction on a row, each as `(condition, its value)`, found the
/// way the graph peels them: `f\{a\}\{b\}`.
fn restrictions(e: &Expr, out: &mut Vec<Condition>) {
    match e {
        Expr::Mul(a, b) => {
            restrictions(a, out);
            restrictions(b, out);
        }
        Expr::Add(_, b) | Expr::Sub(_, b) => restrictions(b, out),
        Expr::Piecewise(branches, None) if branches.len() == 1 => {
            out.push(branches[0].0.clone());
            restrictions(&branches[0].1, out);
        }
        _ => {}
    }
}

/// Literal bounds a chain like `0 ≤ x ≤ 10` puts on `var`.
fn literal_bounds(c: &Condition, var: &str, lo: &mut Option<f64>, hi: &mut Option<f64>) {
    let at = c
        .operands
        .iter()
        .position(|e| matches!(e, Expr::Var(v) if v == var));
    let Some(at) = at else { return };
    let upward = |r: Relation| matches!(r, Relation::Lt | Relation::Le);
    if at > 0 {
        if let Some(v) = c.operands[at - 1].as_literal() {
            if upward(c.relations[at - 1]) {
                *lo = Some(lo.map_or(v, |l: f64| l.max(v)));
            } else {
                *hi = Some(hi.map_or(v, |h: f64| h.min(v)));
            }
        }
    }
    if let Some(next) = c.operands.get(at + 1) {
        if let Some(v) = next.as_literal() {
            if upward(c.relations[at]) {
                *hi = Some(hi.map_or(v, |h: f64| h.min(v)));
            } else {
                *lo = Some(lo.map_or(v, |l: f64| l.max(v)));
            }
        }
    }
}

/// How far dots for ℤ and friends go when nothing bounds them.
const DOTS_REACH: f64 = 1000.0;

/// `y = f(x) \{x ∈ ℤ\}` (or `\{x ∈ A\}`) plots dots: the points (n, f(n))
/// for each member, as Desmos would with a list. Other rows stay as they are.
pub fn discrete_dots(statement: &mut crate::ast::Statement) {
    use crate::ast::Statement;
    let (body, var, flipped) = match statement {
        Statement::Relation(Expr::Var(y), Relation::Eq, body) if y == "y" => {
            (body.clone(), "x", false)
        }
        Statement::Relation(Expr::Var(x), Relation::Eq, body)
            if x == "x" && !body.mentions("x") =>
        {
            (body.clone(), "y", true)
        }
        Statement::Expr(body) if !body.mentions("y") => (body.clone(), "x", false),
        _ => return,
    };
    let mut conditions = Vec::new();
    restrictions(&body, &mut conditions);
    let Some(set) = conditions.iter().find_map(|c| discrete_restriction(c, var)) else {
        return;
    };
    let list = match set {
        Discrete::List(list) => list,
        Discrete::Num(s) => {
            let (mut lo, mut hi) = (None, None);
            for c in &conditions {
                literal_bounds(c, var, &mut lo, &mut hi);
            }
            let floor = match s {
                NumSet::Naturals | NumSet::PosIntegers => 1.0,
                NumSet::Naturals0 | NumSet::NonNegIntegers => 0.0,
                NumSet::Primes => 2.0,
                _ => -DOTS_REACH,
            };
            let ceiling = if s == NumSet::NegIntegers {
                -1.0
            } else {
                DOTS_REACH
            };
            let lo = lo.map_or(floor, |l| l.ceil().max(floor));
            let hi = hi.map_or(ceiling, |h| h.floor().min(ceiling));
            Expr::Range(
                Box::new(Expr::Num(lo)),
                None,
                Box::new(Expr::Num(hi.max(lo))),
            )
        }
    };
    let v = Expr::Var(var.into());
    let point = if flipped {
        Expr::Point(Box::new(body), Box::new(v))
    } else {
        Expr::Point(Box::new(v), Box::new(body))
    };
    *statement = Statement::Expr(Expr::Comprehension(
        Box::new(point),
        vec![(var.into(), list)],
    ));
}

// ---- Sets with known numbers, for Solve ------------------------------------

/// A set whose interval ends and list members are numbers: what Solve keeps
/// its answers in, as for `x^2 = 4, x ∈ ℕ`.
#[derive(Clone, Debug, PartialEq)]
pub enum Domain {
    Num(NumSet),
    Interval {
        lo: f64,
        hi: f64,
        lo_closed: bool,
        hi_closed: bool,
    },
    List(Vec<f64>),
    Empty,
    Op(SetOp, Box<Domain>, Box<Domain>),
}

impl Domain {
    /// Works out a set's numbers with `eval`, which gives a number (or a
    /// list's numbers) for an expression.
    pub fn from_set(
        s: &SetExpr,
        eval: &mut dyn FnMut(&Expr) -> Result<Vec<f64>, String>,
    ) -> Result<Domain, String> {
        let mut one = |e: &Expr| -> Result<f64, String> {
            match eval(e)?.as_slice() {
                [v] => Ok(*v),
                _ => Err("An interval's ends must be numbers".into()),
            }
        };
        Ok(match s {
            SetExpr::Num(n) => Domain::Num(*n),
            SetExpr::Interval {
                lo,
                hi,
                lo_closed,
                hi_closed,
            } => Domain::Interval {
                lo: one(lo)?,
                hi: one(hi)?,
                lo_closed: *lo_closed,
                hi_closed: *hi_closed,
            },
            SetExpr::List(e) => Domain::List(eval(e)?),
            SetExpr::Empty => Domain::Empty,
            SetExpr::Op(op, a, b) => Domain::Op(
                *op,
                Box::new(Domain::from_set(a, eval)?),
                Box::new(Domain::from_set(b, eval)?),
            ),
        })
    }

    pub fn contains(&self, x: f64) -> Result<bool, String> {
        Ok(match self {
            Domain::Num(n) => n.contains(x)?,
            Domain::Interval {
                lo,
                hi,
                lo_closed,
                hi_closed,
            } => {
                let tol = 1e-12 * x.abs().max(1.0);
                (if *lo_closed { x >= lo - tol } else { x > *lo })
                    && (if *hi_closed { x <= hi + tol } else { x < *hi })
            }
            Domain::List(items) => items.iter().any(|v| close(*v, x)),
            Domain::Empty => false,
            Domain::Op(op, a, b) => {
                let (a, b) = (a.contains(x)?, b.contains(x)?);
                match op {
                    SetOp::Union => a || b,
                    SetOp::Inter => a && b,
                    SetOp::Diff => a && !b,
                }
            }
        })
    }

    /// Whether every member is a whole number.
    pub fn integers(&self) -> bool {
        match self {
            Domain::Num(n) => n.is_discrete(),
            Domain::List(items) => items.iter().all(|v| is_integer(*v)),
            Domain::Empty => true,
            Domain::Interval { .. } => false,
            Domain::Op(SetOp::Union, a, b) => a.integers() && b.integers(),
            Domain::Op(SetOp::Inter, a, b) => a.integers() || b.integers(),
            Domain::Op(SetOp::Diff, a, _) => a.integers(),
        }
    }

    /// The smallest and largest a member can be, when the set is bounded.
    pub fn bounds(&self) -> Option<(f64, f64)> {
        match self {
            Domain::Interval { lo, hi, .. } if lo.is_finite() && hi.is_finite() => Some((*lo, *hi)),
            Domain::List(items) if !items.is_empty() => Some((
                items.iter().copied().fold(f64::INFINITY, f64::min),
                items.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            )),
            Domain::Op(SetOp::Inter, a, b) => match (a.bounds(), b.bounds()) {
                (Some((l1, h1)), Some((l2, h2))) => Some((l1.max(l2), h1.min(h2))),
                (x, None) | (None, x) => x,
            },
            Domain::Op(SetOp::Diff, a, _) => a.bounds(),
            Domain::Op(SetOp::Union, a, b) => match (a.bounds(), b.bounds()) {
                (Some((l1, h1)), Some((l2, h2))) => Some((l1.min(l2), h1.max(h2))),
                _ => None,
            },
            _ => None,
        }
    }

    /// The lowest member a whole-number set can start from, for searches.
    pub fn least_integer(&self) -> Option<f64> {
        match self {
            Domain::Num(NumSet::Naturals | NumSet::PosIntegers) => Some(1.0),
            Domain::Num(NumSet::Naturals0 | NumSet::NonNegIntegers) => Some(0.0),
            Domain::Num(NumSet::Primes) => Some(2.0),
            _ => self.bounds().map(|b| b.0.ceil()),
        }
    }
}
