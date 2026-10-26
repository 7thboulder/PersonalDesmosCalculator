//! Sums worked out at analysis time, such as `\sum_{n=1}^{N} v(n, o)` in a
//! Fourier drawing, compiled once into a small stack program over real and
//! complex numbers instead of being read again for every term. The program
//! does exactly what `Env::quick` would, term by term; anything it cannot
//! express makes compiling fail and the sum is worked out the old way.

use crate::analyze::scalar_call;
use crate::builtins::Builtin;
use crate::compile::BinOp;
use std::rc::Rc;

/// A number as its real and imaginary parts.
pub(crate) type Scalar = (f64, f64);

#[derive(Clone, Debug)]
pub(crate) enum Op {
    Const(Scalar),
    /// Pushes a local: a sum's variable or an inlined function's parameter.
    Slot(u16),
    /// Pops into a local.
    Store(u16),
    Neg,
    Bin(BinOp),
    Call(Builtin, u8),
    Real,
    Imag,
    Conj,
    Arg,
    /// Pops a position and pushes that element of list `lists[n]`.
    Index(u16),
    /// Pops the bounds, then sums (or multiplies) sub-program `sub` with
    /// its variable in `slot`.
    Repeat {
        product: bool,
        slot: u16,
        sub: u16,
    },
}

#[derive(Default, Debug)]
pub(crate) struct Program {
    pub ops: Vec<Op>,
    pub subs: Vec<Vec<Op>>,
    pub lists: Vec<Rc<[Scalar]>>,
    pub slots: usize,
}

/// Longest program compiled, so a deeply recursive function gives up
/// rather than growing without end.
pub(crate) const MAX_OPS: usize = 20_000;
/// Most terms in one sum, as `Env::quick` allows.
pub(crate) const MAX_TERMS: f64 = 10_000.0;

impl Program {
    pub fn len(&self) -> usize {
        self.ops.len() + self.subs.iter().map(Vec::len).sum::<usize>()
    }

    /// Runs the program with `slot` set to each value in `from..=to`,
    /// combining the results as a sum or product.
    pub fn repeat(&self, product: bool, slot: u16, from: f64, to: f64) -> Option<Scalar> {
        let mut slots = vec![(f64::NAN, 0.0); self.slots.max(slot as usize + 1)];
        let mut stack = Vec::with_capacity(32);
        self.repeat_ops(&self.ops, product, slot, from, to, &mut slots, &mut stack)
    }

    #[allow(clippy::too_many_arguments)]
    fn repeat_ops(
        &self,
        ops: &[Op],
        product: bool,
        slot: u16,
        from: f64,
        to: f64,
        slots: &mut [Scalar],
        stack: &mut Vec<Scalar>,
    ) -> Option<Scalar> {
        let (op, empty) = if product {
            (BinOp::Mul, 1.0)
        } else {
            (BinOp::Add, 0.0)
        };
        if to < from {
            return Some((empty, 0.0));
        }
        let saved = slots[slot as usize];
        let mut total: Option<Scalar> = None;
        let mut k = from;
        while k <= to {
            slots[slot as usize] = (k, 0.0);
            let term = self.run(ops, slots, stack)?;
            total = Some(match total {
                None => term,
                Some(acc) => scalar_op(op, acc, term)?,
            });
            k += 1.0;
        }
        slots[slot as usize] = saved;
        Some(total.unwrap_or((empty, 0.0)))
    }

    fn run(&self, ops: &[Op], slots: &mut [Scalar], stack: &mut Vec<Scalar>) -> Option<Scalar> {
        let base = stack.len();
        for op in ops {
            match op {
                Op::Const(v) => stack.push(*v),
                Op::Slot(s) => stack.push(slots[*s as usize]),
                Op::Store(s) => slots[*s as usize] = stack.pop()?,
                Op::Neg => {
                    let (a, b) = stack.pop()?;
                    stack.push(if b == 0.0 { (-a, 0.0) } else { (-a, -b) });
                }
                Op::Bin(op) => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    stack.push(scalar_op(*op, a, b)?);
                }
                Op::Call(f, n) => {
                    let at = stack.len().checked_sub(*n as usize)?;
                    let v = scalar_call(*f, &stack[at..])?;
                    stack.truncate(at);
                    stack.push(v);
                }
                Op::Real | Op::Imag | Op::Conj | Op::Arg => {
                    let (a, b) = stack.pop()?;
                    stack.push(match op {
                        Op::Real => (a, 0.0),
                        Op::Imag => (b, 0.0),
                        Op::Conj if b == 0.0 => (a, 0.0),
                        Op::Conj => (a, -b),
                        _ => (Builtin::Arctan.eval(&[b, a]), 0.0),
                    });
                }
                Op::Index(list) => {
                    let (n, im) = stack.pop()?;
                    if im != 0.0 {
                        return None;
                    }
                    let n = n.round();
                    let list = &self.lists[*list as usize];
                    stack.push(if n < 1.0 || n as usize > list.len() {
                        (f64::NAN, 0.0)
                    } else {
                        list[n as usize - 1]
                    });
                }
                Op::Repeat { product, slot, sub } => {
                    let bound = |(v, im): Scalar| (im == 0.0 && v.is_finite()).then_some(v.round());
                    let to = bound(stack.pop()?)?;
                    let from = bound(stack.pop()?)?;
                    if to - from >= MAX_TERMS {
                        return None;
                    }
                    let sub = &self.subs[*sub as usize];
                    let v = self.repeat_ops(sub, *product, *slot, from, to, slots, stack)?;
                    stack.push(v);
                }
            }
        }
        let v = stack.pop()?;
        (stack.len() == base).then_some(v)
    }
}

/// `op` on two numbers, real or complex, as the analyzer works it out.
pub(crate) fn scalar_op(op: BinOp, (a, b): Scalar, (c, d): Scalar) -> Option<Scalar> {
    if b == 0.0 && d == 0.0 {
        return Some((op.apply(a, c), 0.0));
    }
    // A real number's imaginary part is +0 there, whatever its sign here.
    let tidy = |v: f64| if v == 0.0 { 0.0 } else { v };
    let (b, d) = (tidy(b), tidy(d));
    let mul = |(a, b): Scalar, (c, d): Scalar| (a * c - b * d, a * d + b * c);
    Some(match op {
        BinOp::Add => (a + c, b + d),
        BinOp::Sub => (a - c, b - d),
        BinOp::Mul => mul((a, b), (c, d)),
        BinOp::Div => {
            let den = c * c + d * d;
            ((a * c + b * d) / den, (b * c - a * d) / den)
        }
        BinOp::Pow => {
            if d == 0.0 && c.fract() == 0.0 && c.abs() <= 64.0 {
                let mut result = (1.0, 0.0);
                for _ in 0..c.abs() as u32 {
                    let (re, im) = mul(result, (a, b));
                    result = (re, tidy(im));
                }
                if c < 0.0 {
                    let (x, y) = result;
                    let den = x * x + y * y;
                    result = ((1.0 * x + 0.0 * y) / den, tidy((0.0 * x - 1.0 * y) / den));
                }
                return Some(result);
            }
            let exp = |p: f64, q: f64| {
                let scale = Builtin::Exp.eval(&[p]);
                (
                    scale * Builtin::Cos.eval(&[q]),
                    scale * Builtin::Sin.eval(&[q]),
                )
            };
            if a == std::f64::consts::E && b == 0.0 {
                return Some(exp(c, d));
            }
            let ln_r = Builtin::Ln.eval(&[Builtin::Sqrt.eval(&[a * a + b * b])]);
            let theta = Builtin::Arctan.eval(&[b, a]);
            exp(c * ln_r - d * theta, c * theta + d * ln_r)
        }
        _ => return None,
    })
}
