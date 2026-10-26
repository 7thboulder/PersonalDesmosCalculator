//! Derivatives and integrals.
//!
//! Derivatives are taken symbolically on the resolved expression, so
//! `d/dx sin(x^2)` compiles to `cos(x^2) * 2x` and evaluates as fast as
//! anything typed directly. The few functions without a usable closed-form
//! derivative (factorial, gcd and friends) fall back to a central
//! difference built from the same expression. Integrals are numerical:
//! adaptive Gauss-Kronrod, with a change of variable for infinite bounds.

use crate::builtins::Builtin;
use crate::compile::{BinOp, Low};

fn c(v: f64) -> Low {
    Low::Const(v)
}

fn is_const(l: &Low, v: f64) -> bool {
    matches!(l, Low::Const(x) if *x == v)
}

fn neg(a: Low) -> Low {
    match a {
        Low::Const(v) => c(-v),
        Low::Neg(inner) => *inner,
        a => Low::Neg(Box::new(a)),
    }
}

fn add(a: Low, b: Low) -> Low {
    if is_const(&a, 0.0) {
        return b;
    }
    if is_const(&b, 0.0) {
        return a;
    }
    Low::Bin(BinOp::Add, Box::new(a), Box::new(b))
}

fn sub(a: Low, b: Low) -> Low {
    if is_const(&b, 0.0) {
        return a;
    }
    if is_const(&a, 0.0) {
        return neg(b);
    }
    Low::Bin(BinOp::Sub, Box::new(a), Box::new(b))
}

fn mul(a: Low, b: Low) -> Low {
    if is_const(&a, 0.0) || is_const(&b, 0.0) {
        return c(0.0);
    }
    if is_const(&a, 1.0) {
        return b;
    }
    if is_const(&b, 1.0) {
        return a;
    }
    Low::Bin(BinOp::Mul, Box::new(a), Box::new(b))
}

fn div(a: Low, b: Low) -> Low {
    if is_const(&a, 0.0) {
        return c(0.0);
    }
    if is_const(&b, 1.0) {
        return a;
    }
    Low::Bin(BinOp::Div, Box::new(a), Box::new(b))
}

fn pow(a: Low, b: Low) -> Low {
    if is_const(&b, 1.0) {
        return a;
    }
    Low::Bin(BinOp::Pow, Box::new(a), Box::new(b))
}

fn call(f: Builtin, a: Low) -> Low {
    Low::Call(f, vec![a])
}

fn sq(a: &Low) -> Low {
    mul(a.clone(), a.clone())
}

/// The derivative of `e` with respect to the leaf `v` (an `Arg` or a
/// `Hole`).
pub fn derivative(e: &Low, v: &Low) -> Low {
    let d = |x: &Low| derivative(x, v);
    if !mentions(e, v) {
        return c(0.0);
    }
    match e {
        Low::Const(_) => c(0.0),
        Low::Arg(_) | Low::Hole(_) => c(if e == v { 1.0 } else { 0.0 }),
        Low::Neg(a) => neg(d(a)),
        Low::Bin(op, a, b) => {
            let (a, b) = (a.as_ref(), b.as_ref());
            match op {
                BinOp::Add => add(d(a), d(b)),
                BinOp::Sub => sub(d(a), d(b)),
                BinOp::Mul => add(mul(d(a), b.clone()), mul(a.clone(), d(b))),
                BinOp::Div => div(sub(mul(d(a), b.clone()), mul(a.clone(), d(b))), sq(b)),
                BinOp::Pow => {
                    if !mentions(b, v) {
                        // b a^(b-1) a'
                        mul(mul(b.clone(), pow(a.clone(), sub(b.clone(), c(1.0)))), d(a))
                    } else if !mentions(a, v) {
                        // a^b ln(a) b'
                        mul(mul(e.clone(), call(Builtin::Ln, a.clone())), d(b))
                    } else {
                        // a^b (b' ln a + b a'/a)
                        mul(
                            e.clone(),
                            add(
                                mul(d(b), call(Builtin::Ln, a.clone())),
                                div(mul(b.clone(), d(a)), a.clone()),
                            ),
                        )
                    }
                }
                // Comparisons are flat wherever they are defined.
                BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq => c(0.0),
            }
        }
        Low::Select(cond, a, b) => Low::Select(cond.clone(), Box::new(d(a)), Box::new(d(b))),
        Low::Call(f, args) => call_derivative(e, *f, args, v),
        Low::Integral { hole, body, lo, hi } => {
            // Leibniz: f(hi) hi' - f(lo) lo' + the integral of ∂f/∂v.
            let at = |bound: &Low| body.fill(*hole, bound);
            let ends = sub(mul(at(hi), d(hi)), mul(at(lo), d(lo)));
            let inner = derivative(body, v);
            if matches!(inner, Low::Const(z) if z == 0.0) {
                ends
            } else {
                add(
                    ends,
                    Low::Integral {
                        hole: *hole,
                        body: Box::new(inner),
                        lo: lo.clone(),
                        hi: hi.clone(),
                    },
                )
            }
        }
    }
}

fn mentions(e: &Low, v: &Low) -> bool {
    match v {
        Low::Arg(a) => e.uses(*a),
        Low::Hole(h) => e.uses_hole(*h),
        _ => false,
    }
}

fn call_derivative(e: &Low, f: Builtin, args: &[Low], v: &Low) -> Low {
    use Builtin::*;
    let d = |x: &Low| derivative(x, v);
    let one = |outer: Low| -> Low { mul(outer, d(&args[0])) };
    let a = || args[0].clone();
    let recip_sqrt = |inside: Low| div(c(1.0), call(Sqrt, inside));
    match (f, args.len()) {
        (Sin, 1) => one(call(Cos, a())),
        (Cos, 1) => one(neg(call(Sin, a()))),
        (Tan, 1) => one(div(c(1.0), sq(&call(Cos, a())))),
        (Csc, 1) => one(neg(mul(call(Csc, a()), call(Cot, a())))),
        (Sec, 1) => one(mul(call(Sec, a()), call(Tan, a()))),
        (Cot, 1) => one(neg(sq(&call(Csc, a())))),
        (Arcsin, 1) => one(recip_sqrt(sub(c(1.0), sq(&a())))),
        (Arccos, 1) => one(neg(recip_sqrt(sub(c(1.0), sq(&a()))))),
        (Arctan, 1) => one(div(c(1.0), add(c(1.0), sq(&a())))),
        (Arctan, 2) => {
            // atan2(y, x): (x y' - y x') / (x² + y²)
            let (y, x) = (&args[0], &args[1]);
            div(
                sub(mul(x.clone(), d(y)), mul(y.clone(), d(x))),
                add(sq(x), sq(y)),
            )
        }
        (Arccsc, 1) => one(neg(div(
            c(1.0),
            mul(call(Abs, a()), call(Sqrt, sub(sq(&a()), c(1.0)))),
        ))),
        (Arcsec, 1) => one(div(
            c(1.0),
            mul(call(Abs, a()), call(Sqrt, sub(sq(&a()), c(1.0)))),
        )),
        (Arccot, 1) => one(neg(div(c(1.0), add(c(1.0), sq(&a()))))),
        (Sinh, 1) => one(call(Cosh, a())),
        (Cosh, 1) => one(call(Sinh, a())),
        (Tanh, 1) => one(div(c(1.0), sq(&call(Cosh, a())))),
        (Csch, 1) => one(neg(mul(call(Csch, a()), call(Coth, a())))),
        (Sech, 1) => one(neg(mul(call(Sech, a()), call(Tanh, a())))),
        (Coth, 1) => one(neg(div(c(1.0), sq(&call(Sinh, a()))))),
        (Arcsinh, 1) => one(recip_sqrt(add(sq(&a()), c(1.0)))),
        (Arccosh, 1) => one(recip_sqrt(sub(sq(&a()), c(1.0)))),
        (Arctanh, 1) | (Arccoth, 1) => one(div(c(1.0), sub(c(1.0), sq(&a())))),
        (Arccsch, 1) => one(neg(div(
            c(1.0),
            mul(call(Abs, a()), call(Sqrt, add(c(1.0), sq(&a())))),
        ))),
        (Arcsech, 1) => one(neg(div(
            c(1.0),
            mul(a(), call(Sqrt, sub(c(1.0), sq(&a())))),
        ))),
        (Ln, 1) => one(div(c(1.0), a())),
        (Log, 1) => one(div(c(1.0), mul(a(), c(std::f64::consts::LN_10)))),
        (LogBase, 2) => derivative(
            &div(call(Ln, args[1].clone()), call(Ln, args[0].clone())),
            v,
        ),
        (Exp, 1) => one(e.clone()),
        (Sqrt, 1) => one(div(c(1.0), mul(c(2.0), e.clone()))),
        (Root, 2) => derivative(&pow(args[1].clone(), div(c(1.0), args[0].clone())), v),
        (Abs, 1) => one(call(Sign, a())),
        // Steps are flat between their jumps, as Desmos shows them.
        (Floor, _) | (Ceil, _) | (Round, _) | (Sign, _) => c(0.0),
        (Mod, 2) => sub(
            d(&args[0]),
            mul(
                d(&args[1]),
                call(Floor, div(args[0].clone(), args[1].clone())),
            ),
        ),
        (Min, _) | (Max, _) if !args.is_empty() => {
            // The derivative of whichever argument is the extreme one.
            let op = if f == Min { BinOp::Le } else { BinOp::Ge };
            let mut best = args[0].clone();
            let mut slope = d(&args[0]);
            for arg in &args[1..] {
                let keep = Low::Bin(op, Box::new(best.clone()), Box::new(arg.clone()));
                slope = Low::Select(Box::new(keep.clone()), Box::new(slope), Box::new(d(arg)));
                best = Low::Call(f, vec![best, arg.clone()]);
            }
            slope
        }
        (Total, _) => args.iter().map(d).fold(c(0.0), add),
        (Mean, n) if n > 0 => div(args.iter().map(d).fold(c(0.0), add), c(n as f64)),
        (Length, _) => c(0.0),
        _ => numeric_derivative(e, v),
    }
}

/// A central difference of `e` around `v`, for functions with no
/// closed-form derivative here.
fn numeric_derivative(e: &Low, v: &Low) -> Low {
    // A step that scales with the variable keeps precision at any size.
    let h = mul(
        c(1e-6),
        Low::Call(Builtin::Max, vec![c(1.0), call(Builtin::Abs, v.clone())]),
    );
    let shifted = |sign: f64| {
        let at = add(v.clone(), mul(c(sign), h.clone()));
        e.map_leaves(&|l| (l == v).then(|| at.clone()))
    };
    div(sub(shifted(1.0), shifted(-1.0)), mul(c(2.0), h.clone()))
}

/// 15-point Kronrod nodes on [0, 1] (mirrored for [-1, 0]), the 7-point
/// Gauss rule embedded at the odd indices.
const XGK: [f64; 8] = [
    0.991_455_371_120_812_6,
    0.949_107_912_342_758_5,
    0.864_864_423_359_769_1,
    0.741_531_185_599_394_4,
    0.586_087_235_467_691_1,
    0.405_845_151_377_397_2,
    0.207_784_955_007_898_5,
    0.0,
];
const WGK: [f64; 8] = [
    0.022_935_322_010_529_225,
    0.063_092_092_629_978_55,
    0.104_790_010_322_250_18,
    0.140_653_259_715_525_92,
    0.169_004_726_639_267_9,
    0.190_350_578_064_785_4,
    0.204_432_940_075_298_9,
    0.209_482_141_084_727_83,
];
const WG: [f64; 4] = [
    0.129_484_966_168_869_7,
    0.279_705_391_489_276_7,
    0.381_830_050_505_118_9,
    0.417_959_183_673_469_4,
];
/// Most intervals one integral may split into.
const MAX_INTERVALS: usize = 400;

/// One Gauss-Kronrod step on [a, b]: the estimate and its error.
fn kronrod(f: &mut dyn FnMut(f64) -> f64, a: f64, b: f64) -> (f64, f64) {
    let (mid, half) = ((a + b) / 2.0, (b - a) / 2.0);
    // A function can be undefined at a single point, like x/|x| at 0, and
    // Desmos still integrates it. A node that lands on such a point is moved
    // a hair to either side; only a function undefined around the node too
    // makes the integral undefined.
    let mut f = |x: f64| {
        let y = f(x);
        if !y.is_nan() {
            return y;
        }
        let nudge = half * 1e-9;
        let right = f(x + nudge);
        if right.is_nan() {
            f(x - nudge)
        } else {
            right
        }
    };
    let centre = f(mid);
    let mut k = centre * WGK[7];
    let mut g = centre * WG[3];
    for i in 0..7 {
        let dx = half * XGK[i];
        let pair = f(mid - dx) + f(mid + dx);
        k += WGK[i] * pair;
        if i % 2 == 1 {
            g += WG[i / 2] * pair;
        }
    }
    (k * half, ((k - g) * half).abs())
}

/// The integral of `f` from `a` to `b`, either of which may be infinite.
pub fn integrate(f: &mut dyn FnMut(f64) -> f64, a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == b {
        return 0.0;
    }
    if a > b {
        return -integrate(f, b, a);
    }
    match (a.is_finite(), b.is_finite()) {
        (true, true) => adaptive(f, a, b),
        // [a, ∞): t = a + u/(1-u) for u in [0, 1).
        (true, false) => adaptive(
            &mut |u| {
                let w = 1.0 - u;
                f(a + u / w) / (w * w)
            },
            0.0,
            1.0,
        ),
        // (-∞, b]: t = b - u/(1-u).
        (false, true) => adaptive(
            &mut |u| {
                let w = 1.0 - u;
                f(b - u / w) / (w * w)
            },
            0.0,
            1.0,
        ),
        (false, false) => integrate(f, a, 0.0) + integrate(f, 0.0, b),
    }
}

/// Splits the interval with the largest error until the total error is
/// small or the interval budget runs out.
fn adaptive(f: &mut dyn FnMut(f64) -> f64, a: f64, b: f64) -> f64 {
    let (total, err) = kronrod(f, a, b);
    let mut pieces = vec![Piece {
        lo: a,
        hi: b,
        value: total,
        error: err,
    }];
    let (mut sum, mut error) = (total, err);
    // Pieces are a max-heap on error so the worst one is found in log time.
    while sum.is_finite() && error > 1e-11_f64.max(1e-9 * sum.abs()) && pieces.len() < MAX_INTERVALS
    {
        let worst = pop_worst(&mut pieces);
        let mid = (worst.lo + worst.hi) / 2.0;
        let (left, el) = kronrod(f, worst.lo, mid);
        let (right, er) = kronrod(f, mid, worst.hi);
        sum += left + right - worst.value;
        error += el + er - worst.error;
        push(
            &mut pieces,
            Piece {
                lo: worst.lo,
                hi: mid,
                value: left,
                error: el,
            },
        );
        push(
            &mut pieces,
            Piece {
                lo: mid,
                hi: worst.hi,
                value: right,
                error: er,
            },
        );
    }
    // Re-add from scratch so the running sum's rounding does not linger.
    pieces.iter().map(|p| p.value).sum()
}

struct Piece {
    lo: f64,
    hi: f64,
    value: f64,
    error: f64,
}

fn push(heap: &mut Vec<Piece>, piece: Piece) {
    heap.push(piece);
    let mut i = heap.len() - 1;
    while i > 0 {
        let parent = (i - 1) / 2;
        if heap[parent].error >= heap[i].error {
            break;
        }
        heap.swap(parent, i);
        i = parent;
    }
}

fn pop_worst(heap: &mut Vec<Piece>) -> Piece {
    let worst = heap.swap_remove(0);
    let mut i = 0;
    loop {
        let (l, r) = (2 * i + 1, 2 * i + 2);
        let mut big = i;
        if l < heap.len() && heap[l].error > heap[big].error {
            big = l;
        }
        if r < heap.len() && heap[r].error > heap[big].error {
            big = r;
        }
        if big == i {
            return worst;
        }
        heap.swap(i, big);
        i = big;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::{Arg, Program};

    fn x() -> Low {
        Low::Arg(Arg::X)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-8 * (1.0 + b.abs())
    }

    fn slope_at(e: &Low, at: f64) -> f64 {
        Program::compile(&derivative(e, &x())).unwrap().eval(at)
    }

    #[test]
    fn symbolic_derivatives_match_differences() {
        let cases = [
            call(Builtin::Sin, mul(x(), x())),
            div(call(Builtin::Ln, x()), add(x(), c(2.0))),
            pow(x(), x()),
            call(Builtin::Arctan, mul(c(3.0), x())),
            call(Builtin::Sqrt, add(sq(&x()), c(1.0))),
            Low::Call(Builtin::LogBase, vec![c(2.0), x()]),
            call(Builtin::Sech, x()),
            Low::Call(Builtin::Max, vec![x(), neg(x()), c(0.5)]),
        ];
        for e in &cases {
            let p = Program::compile(e).unwrap();
            for at in [0.7, 1.3, 2.9] {
                let h = 1e-6;
                let numeric = (p.eval(at + h) - p.eval(at - h)) / (2.0 * h);
                let symbolic = slope_at(e, at);
                assert!(
                    (numeric - symbolic).abs() < 1e-5,
                    "{e:?} at {at}: {numeric} vs {symbolic}"
                );
            }
        }
    }

    #[test]
    fn real_powers_of_negative_numbers() {
        // d/dx x^(1/3) = (1/3) x^(-2/3), real for negative x as in Desmos.
        let e = pow(x(), div(c(1.0), c(3.0)));
        assert!(close(slope_at(&e, -8.0), 1.0 / 12.0));
    }

    #[test]
    fn factorial_falls_back_to_a_difference() {
        let e = Low::Call(Builtin::Factorial, vec![x()]);
        // d/dx x! at 1 is 1 - γ.
        assert!((slope_at(&e, 1.0) - 0.422_784_335_098_467).abs() < 1e-5);
    }

    #[test]
    fn integrals() {
        let mut sq = |t: f64| t * t;
        assert!(close(integrate(&mut sq, 0.0, 3.0), 9.0));
        assert!(close(integrate(&mut sq, 3.0, 0.0), -9.0));
        let mut gauss = |t: f64| (-t * t).exp();
        assert!(close(
            integrate(&mut gauss, f64::NEG_INFINITY, f64::INFINITY),
            std::f64::consts::PI.sqrt()
        ));
        let mut decay = |t: f64| (-t).exp();
        assert!(close(integrate(&mut decay, 0.0, f64::INFINITY), 1.0));
        let mut wiggle = |t: f64| (t * t).sin();
        assert!(close(
            integrate(&mut wiggle, 0.0, 10.0),
            0.583_670_899_929_623_4
        ));
        let mut root = |t: f64| 1.0 / t.sqrt();
        assert!((integrate(&mut root, 0.0, 1.0) - 2.0).abs() < 1e-4);
    }

    #[test]
    fn isolated_undefined_points_are_stepped_over() {
        let pi = std::f64::consts::PI;
        let mut sign = |t: f64| -t / t.abs();
        assert!(integrate(&mut sign, -pi, pi).abs() < 1e-9);
        assert!(close(integrate(&mut sign, -pi, 2.0 * pi), -pi));
        let mut odd = |t: f64| -t / t.abs() * t.sin();
        assert!(close(integrate(&mut odd, -pi, pi), -4.0));
        // Undefined over a whole stretch stays undefined.
        let mut root = |t: f64| t.sqrt();
        assert!(integrate(&mut root, -1.0, 1.0).is_nan());
    }

    #[test]
    fn leibniz_rule_for_variable_bounds() {
        // d/dx ∫_0^x t² dt = x².
        let e = Low::Integral {
            hole: 7,
            body: Box::new(sq(&Low::Hole(7))),
            lo: Box::new(c(0.0)),
            hi: Box::new(x()),
        };
        assert!(close(Program::compile(&e).unwrap().eval(2.0), 8.0 / 3.0));
        assert!(close(slope_at(&e, 3.0), 9.0));
    }
}
