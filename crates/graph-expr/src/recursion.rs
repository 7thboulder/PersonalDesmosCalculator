//! Functions that call themselves, as Desmos allows:
//! `f(n) = {n = 0: 1, n f(n-1)}`.
//!
//! Ordinary functions are inlined where they are called, which can't end
//! for a function that calls itself. A recursive function (with every
//! function that calls it back) is lowered once into a [`Recursion`], whose
//! calls are worked out when the program runs: a piecewise takes only the
//! branch its condition picks, so a base case stops the calls, and results
//! are remembered, so `f(n-1) + f(n-2)` takes n steps rather than 2ⁿ.

use std::cell::Cell;
use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use crate::compile::Low;

/// How deep calls may go before the call gives up, as Desmos's limit does.
pub const MAX_DEPTH: usize = 10_000;
/// How many calls one evaluation may make, so a function that branches
/// without end can't hang the app.
const MAX_CALLS: usize = 2_000_000;
/// Remembered results kept before the memory is cleared.
const MAX_REMEMBERED: usize = 1 << 20;

/// One function of a group: its parameters (as the holes its body reads)
/// and its body, in which calls within the group are [`Low::SelfCall`].
#[derive(Clone, Debug, PartialEq)]
pub struct Body {
    pub holes: Vec<u32>,
    pub low: Low,
}

/// Functions that call each other (or one that calls itself).
pub struct Recursion {
    pub names: Vec<String>,
    pub bodies: Vec<Body>,
    memo: Mutex<HashMap<(u16, Vec<u64>), f64>>,
}

impl Recursion {
    pub fn new(names: Vec<String>, bodies: Vec<Body>) -> Recursion {
        Recursion {
            names,
            bodies,
            memo: Mutex::new(HashMap::new()),
        }
    }
}

impl PartialEq for Recursion {
    fn eq(&self, other: &Recursion) -> bool {
        self.names == other.names && self.bodies == other.bodies
    }
}

impl fmt::Debug for Recursion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Recursion")
            .field("names", &self.names)
            .field("bodies", &self.bodies)
            .finish()
    }
}

thread_local! {
    static DEPTH: Cell<usize> = const { Cell::new(0) };
    static CALLS: Cell<usize> = const { Cell::new(0) };
    /// Set when a call gave up for going too deep or too long.
    static GAVE_UP: Cell<Option<Limit>> = const { Cell::new(None) };
}

/// Why a call gave up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    Depth,
    Calls,
}

impl Recursion {
    /// Function `f` of the group at `args`; undefined when the calls go
    /// too deep.
    pub fn call(&self, f: u16, args: &[f64]) -> f64 {
        let outermost = DEPTH.with(|d| d.get()) == 0;
        if outermost {
            CALLS.with(|c| c.set(0));
        }
        let v = self.call_inner(f, args);
        if outermost && GAVE_UP.with(|g| g.get()).is_some() {
            // Results worked out on the way are undefined only because of
            // the limit, so they must not be remembered.
            self.forget();
        }
        v
    }

    /// Like [`Recursion::call`], saying why it gave up when it did.
    pub fn call_checked(&self, f: u16, args: &[f64]) -> Result<f64, Limit> {
        GAVE_UP.with(|g| g.set(None));
        let v = self.call(f, args);
        match GAVE_UP.with(|g| g.take()) {
            Some(limit) => Err(limit),
            None => Ok(v),
        }
    }

    fn forget(&self) {
        if let Ok(mut m) = self.memo.lock() {
            m.clear();
        }
    }

    fn call_inner(&self, f: u16, args: &[f64]) -> f64 {
        if args.iter().any(|a| a.is_nan()) {
            return f64::NAN;
        }
        let Some(body) = self.bodies.get(f as usize) else {
            return f64::NAN;
        };
        if body.holes.len() != args.len() {
            return f64::NAN;
        }
        let key = (f, args.iter().map(|a| a.to_bits()).collect::<Vec<_>>());
        if let Some(v) = self.memo.lock().ok().and_then(|m| m.get(&key).copied()) {
            return v;
        }
        let depth = DEPTH.with(|d| d.get());
        let calls = CALLS.with(|c| {
            c.set(c.get() + 1);
            c.get()
        });
        if depth >= MAX_DEPTH || calls >= MAX_CALLS {
            GAVE_UP.with(|g| {
                if g.get().is_none() {
                    g.set(Some(if depth >= MAX_DEPTH {
                        Limit::Depth
                    } else {
                        Limit::Calls
                    }))
                }
            });
            return f64::NAN;
        }
        if GAVE_UP.with(|g| g.get()).is_some() {
            return f64::NAN;
        }
        DEPTH.with(|d| d.set(depth + 1));
        let mut env: Vec<(u32, f64)> = body
            .holes
            .iter()
            .copied()
            .zip(args.iter().copied())
            .collect();
        // Deep calls run on a stack that grows as needed.
        let v = stacker::maybe_grow(64 * 1024, 2 * 1024 * 1024, || {
            self.eval(&body.low, &mut env)
        });
        DEPTH.with(|d| d.set(depth));
        if GAVE_UP.with(|g| g.get()).is_none() {
            if let Ok(mut m) = self.memo.lock() {
                if m.len() >= MAX_REMEMBERED {
                    m.clear();
                }
                m.insert(key, v);
            }
        }
        v
    }

    fn eval(&self, e: &Low, env: &mut Vec<(u32, f64)>) -> f64 {
        match e {
            Low::Const(v) => *v,
            Low::Hole(h) => env
                .iter()
                .rev()
                .find(|(k, _)| k == h)
                .map_or(f64::NAN, |(_, v)| *v),
            Low::Arg(_) => f64::NAN,
            Low::Neg(a) => -self.eval(a, env),
            Low::Bin(op, a, b) => {
                let a = self.eval(a, env);
                let b = self.eval(b, env);
                op.apply(a, b)
            }
            Low::Call(f, args) => {
                let values: Vec<f64> = args.iter().map(|a| self.eval(a, env)).collect();
                f.eval(&values)
            }
            // Only the branch the condition picks is worked out, so a base
            // case stops the calls.
            Low::Select(c, a, b) => {
                let c = self.eval(c, env);
                if c.is_nan() {
                    f64::NAN
                } else if c != 0.0 {
                    self.eval(a, env)
                } else {
                    self.eval(b, env)
                }
            }
            Low::Integral {
                how,
                hole,
                body,
                lo,
                hi,
            } => {
                let (lo, hi) = (self.eval(lo, env), self.eval(hi, env));
                let hole = *hole;
                crate::compile::accumulate(
                    *how,
                    &mut |t| {
                        env.push((hole, t));
                        let v = self.eval(body, env);
                        env.pop();
                        v
                    },
                    lo,
                    hi,
                    1 << 12,
                )
            }
            Low::SelfCall(f, args) => {
                let values: Vec<f64> = args.iter().map(|a| self.eval(a, env)).collect();
                self.call_inner(*f, &values)
            }
            Low::Recur(other, f, args) => {
                let values: Vec<f64> = args.iter().map(|a| self.eval(a, env)).collect();
                other.call_inner(*f, &values)
            }
        }
    }
}

impl Limit {
    pub fn message(self, name: &str) -> String {
        match self {
            Limit::Depth => format!(
                "'{name}' calls itself more than {MAX_DEPTH} times deep. Give it a base case that stops the calls, like {{n = 0: 1, n {name}(n - 1)}}"
            ),
            Limit::Calls => format!("'{name}' makes too many calls to work out"),
        }
    }
}
