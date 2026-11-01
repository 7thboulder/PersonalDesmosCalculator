//! Desmos's built-in functions and the numeric rules they follow.

use std::f64::consts::{FRAC_PI_2, PI};

use crate::special;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Builtin {
    Sin,
    Cos,
    Tan,
    Csc,
    Sec,
    Cot,
    Arcsin,
    Arccos,
    Arctan,
    Arccsc,
    Arcsec,
    Arccot,
    Sinh,
    Cosh,
    Tanh,
    Csch,
    Sech,
    Coth,
    Arcsinh,
    Arccosh,
    Arctanh,
    Arccsch,
    Arcsech,
    Arccoth,
    Ln,
    Log,
    LogBase,
    Exp,
    Sqrt,
    Root,
    Abs,
    Floor,
    Ceil,
    Round,
    Sign,
    Mod,
    Min,
    Max,
    Gcd,
    Lcm,
    NCr,
    NPr,
    Mean,
    Median,
    Total,
    Length,
    Factorial,
    // Special functions, beyond Desmos.
    Gamma,
    Digamma,
    Beta,
    Erf,
    Erfc,
    Erfi,
    Zeta,
    BesselJ,
    BesselY,
    BesselI,
    BesselK,
    AiryAi,
    AiryBi,
    Si,
    Ci,
    Ei,
    Li,
    LambertW,
    Polylog,
    EllipticK,
    EllipticE,
    /// The real and imaginary parts of Γ and ζ at a complex number,
    /// for complex rows. They take (re, im) and are never typed.
    GammaRe,
    GammaIm,
    ZetaRe,
    ZetaIm,
}

/// How many arguments a function takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arity {
    pub min: usize,
    pub max: usize,
}

use Builtin::*;

const NAMES: &[(&str, Builtin)] = &[
    ("sin", Sin),
    ("cos", Cos),
    ("tan", Tan),
    ("csc", Csc),
    ("sec", Sec),
    ("cot", Cot),
    ("arcsin", Arcsin),
    ("arccos", Arccos),
    ("arctan", Arctan),
    ("arccsc", Arccsc),
    ("arcsec", Arcsec),
    ("arccot", Arccot),
    ("sinh", Sinh),
    ("cosh", Cosh),
    ("tanh", Tanh),
    ("csch", Csch),
    ("sech", Sech),
    ("coth", Coth),
    ("arcsinh", Arcsinh),
    ("arccosh", Arccosh),
    ("arctanh", Arctanh),
    ("arccsch", Arccsch),
    ("arcsech", Arcsech),
    ("arccoth", Arccoth),
    ("arsinh", Arcsinh),
    ("arcosh", Arccosh),
    ("artanh", Arctanh),
    ("ln", Ln),
    ("log", Log),
    ("exp", Exp),
    ("abs", Abs),
    ("floor", Floor),
    ("ceil", Ceil),
    ("round", Round),
    ("sign", Sign),
    ("sgn", Sign),
    ("mod", Mod),
    ("min", Min),
    ("max", Max),
    ("gcd", Gcd),
    ("lcm", Lcm),
    ("nCr", NCr),
    ("nPr", NPr),
    ("mean", Mean),
    ("median", Median),
    ("total", Total),
    ("length", Length),
    ("erf", Erf),
    ("erfc", Erfc),
    ("erfi", Erfi),
    ("Si", Si),
    ("Ci", Ci),
    ("Ei", Ei),
    ("li", Li),
    ("Ai", AiryAi),
    ("Bi", AiryBi),
    // Li_s(x), with the order as a subscript.
    ("Li", Polylog),
];

/// Special functions written with a letter rather than a name, such as
/// Γ(x) or J_n(x). They are only functions when the name isn't defined in
/// the graph, and `args` is how many arguments each takes as written.
pub fn from_letter(name: &str, args: usize) -> Option<Builtin> {
    Some(match (name, args) {
        ("Gamma", 1) => Gamma,
        ("psi", 1) => Digamma,
        ("B", 2) => Beta,
        ("zeta", 1) => Zeta,
        ("W", 1) => LambertW,
        ("K", 1) => EllipticK,
        ("E", 1) => EllipticE,
        _ => return None,
    })
}

/// The Bessel function a subscripted letter names: J, Y, I or K.
pub fn bessel(letter: &str) -> Option<Builtin> {
    Some(match letter {
        "J" => BesselJ,
        "Y" => BesselY,
        "I" => BesselI,
        "K" => BesselK,
        _ => return None,
    })
}

pub fn from_name(name: &str) -> Option<Builtin> {
    NAMES.iter().find(|(n, _)| *n == name).map(|(_, b)| *b)
}

pub fn is_operator_name(name: &str) -> bool {
    from_name(name).is_some()
}

impl Builtin {
    pub fn name(self) -> &'static str {
        match self {
            Sqrt => "sqrt",
            Root => "nthroot",
            LogBase => "log",
            Factorial => "factorial",
            Gamma | GammaRe | GammaIm => "Γ",
            Digamma => "ψ",
            Beta => "B",
            Zeta | ZetaRe | ZetaIm => "ζ",
            BesselJ => "J",
            BesselY => "Y",
            BesselI => "I",
            BesselK => "K",
            LambertW => "W",
            EllipticK => "K",
            EllipticE => "E",
            other => NAMES
                .iter()
                .find(|(_, b)| *b == other)
                .map(|(n, _)| *n)
                .unwrap_or("?"),
        }
    }

    pub fn arity(self) -> Arity {
        let (min, max) = match self {
            LogBase | Root | Mod | NCr | NPr | Beta | BesselJ | BesselY | BesselI | BesselK | Polylog
            | GammaRe | GammaIm | ZetaRe | ZetaIm => (2, 2),
            Arctan | Round => (1, 2),
            Min | Max | Gcd | Lcm | Mean | Median | Total | Length => (1, usize::MAX),
            _ => (1, 1),
        };
        Arity { min, max }
    }

    /// `\sin^{-1}` means arcsin, as in Desmos.
    /// Functions that take a whole list, such as `total`, rather than being
    /// applied to each element.
    pub fn takes_list(self) -> bool {
        matches!(self, Min | Max | Gcd | Lcm | Mean | Median | Total | Length)
    }

    pub fn inverse(self) -> Option<Builtin> {
        Some(match self {
            Sin => Arcsin,
            Cos => Arccos,
            Tan => Arctan,
            Csc => Arccsc,
            Sec => Arcsec,
            Cot => Arccot,
            Sinh => Arcsinh,
            Cosh => Arccosh,
            Tanh => Arctanh,
            Csch => Arccsch,
            Sech => Arcsech,
            Coth => Arccoth,
            _ => return None,
        })
    }

    pub fn eval(self, a: &[f64]) -> f64 {
        let x = a.first().copied().unwrap_or(f64::NAN);
        match self {
            Sin => x.sin(),
            Cos => x.cos(),
            Tan => x.tan(),
            Csc => 1.0 / x.sin(),
            Sec => 1.0 / x.cos(),
            Cot => 1.0 / x.tan(),
            Arcsin => x.asin(),
            Arccos => x.acos(),
            Arctan if a.len() == 2 => x.atan2(a[1]),
            Arctan => x.atan(),
            Arccsc => (1.0 / x).asin(),
            Arcsec => (1.0 / x).acos(),
            Arccot => FRAC_PI_2 - x.atan(),
            Sinh => x.sinh(),
            Cosh => x.cosh(),
            Tanh => x.tanh(),
            Csch => 1.0 / x.sinh(),
            Sech => 1.0 / x.cosh(),
            Coth => 1.0 / x.tanh(),
            Arcsinh => x.asinh(),
            Arccosh => x.acosh(),
            Arctanh => x.atanh(),
            Arccsch => (1.0 / x).asinh(),
            Arcsech => (1.0 / x).acosh(),
            Arccoth => (1.0 / x).atanh(),
            Ln => x.ln(),
            Log => x.log10(),
            LogBase => a[1].ln() / x.ln(),
            Exp => x.exp(),
            Sqrt => x.sqrt(),
            Root => nth_root(a[1], x),
            Abs => x.abs(),
            Floor => x.floor(),
            Ceil => x.ceil(),
            Round if a.len() == 2 => {
                let scale = 10f64.powf(a[1].round());
                (x * scale).round() / scale
            }
            Round => x.round(),
            Sign => {
                if x > 0.0 {
                    1.0
                } else if x < 0.0 {
                    -1.0
                } else {
                    x // 0 or NaN
                }
            }
            Mod => x - a[1] * (x / a[1]).floor(),
            Min => a.iter().copied().fold(f64::INFINITY, f64::min),
            Max => a.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            Gcd => integer_fold(a, gcd),
            Lcm => integer_fold(a, |p, q| {
                if p == 0.0 || q == 0.0 {
                    0.0
                } else {
                    (p / gcd(p, q) * q).abs()
                }
            }),
            NCr => choose(x, a[1]),
            NPr => permutations(x, a[1]),
            Mean => a.iter().sum::<f64>() / a.len() as f64,
            Median => median(a),
            Total => a.iter().sum(),
            Length => a.len() as f64,
            Factorial => factorial(x),
            Gamma => special::gamma(x),
            Digamma => special::digamma(x),
            Beta => special::beta(x, a[1]),
            Erf => special::erf(x),
            Erfc => special::erfc(x),
            Erfi => special::erfi(x),
            Zeta => special::zeta(x),
            BesselJ => special::bessel_j(x, a[1]),
            BesselY => special::bessel_y(x, a[1]),
            BesselI => special::bessel_i(x, a[1]),
            BesselK => special::bessel_k(x, a[1]),
            AiryAi => special::airy_ai(x),
            AiryBi => special::airy_bi(x),
            Si => special::si(x),
            Ci => special::ci(x),
            Ei => special::ei(x),
            Li => special::li(x),
            LambertW => special::lambert_w(x),
            Polylog => special::polylog(x, a[1]),
            EllipticK => special::elliptic_k(x),
            EllipticE => special::elliptic_e(x),
            GammaRe => special::gamma_complex(special::C::new(x, a[1])).re,
            GammaIm => special::gamma_complex(special::C::new(x, a[1])).im,
            ZetaRe => special::zeta_complex(special::C::new(x, a[1])).re,
            ZetaIm => special::zeta_complex(special::C::new(x, a[1])).im,
        }
    }
}

/// x^y with Desmos's real-number rules: a negative base with an exponent
/// that is a fraction with an odd denominator gives a real answer, so
/// (-8)^(1/3) is -2 and x^(1/3) graphs for negative x.
pub fn pow(x: f64, y: f64) -> f64 {
    if x >= 0.0 || y.fract() == 0.0 || !y.is_finite() {
        return x.powf(y);
    }
    match small_fraction(y) {
        Some((p, q)) if q % 2 == 1 => {
            let magnitude = (-x).powf(y);
            if p % 2 == 0 {
                magnitude
            } else {
                -magnitude
            }
        }
        _ => f64::NAN,
    }
}

/// The n-th root, real for negative x when n is an odd integer.
fn nth_root(x: f64, n: f64) -> f64 {
    if x < 0.0 && n.fract() == 0.0 && (n as i64) % 2 != 0 {
        -(-x).powf(1.0 / n)
    } else {
        x.powf(1.0 / n)
    }
}

/// Finds p/q equal to `y` with q below 1000, if there is one.
fn small_fraction(y: f64) -> Option<(i64, i64)> {
    for q in 1..1000i64 {
        let p = (y * q as f64).round();
        if (p / q as f64 - y).abs() <= 1e-12 * y.abs().max(1.0) {
            return Some((p as i64, q));
        }
    }
    None
}

fn integer_fold(a: &[f64], f: impl Fn(f64, f64) -> f64) -> f64 {
    if a.iter().any(|v| v.fract() != 0.0 || !v.is_finite()) {
        return f64::NAN;
    }
    a.iter()
        .copied()
        .reduce(f)
        .map(f64::abs)
        .unwrap_or(f64::NAN)
}

fn gcd(mut p: f64, mut q: f64) -> f64 {
    p = p.abs();
    q = q.abs();
    while q > 0.0 {
        (p, q) = (q, p % q);
    }
    p
}

fn choose(n: f64, k: f64) -> f64 {
    if n.fract() != 0.0 || k.fract() != 0.0 || n < 0.0 {
        return f64::NAN;
    }
    if k < 0.0 || k > n {
        return 0.0;
    }
    let k = k.min(n - k);
    (1..=k as u64)
        .fold(1.0, |acc, i| acc * (n - k + i as f64) / i as f64)
        .round()
}

fn permutations(n: f64, k: f64) -> f64 {
    if n.fract() != 0.0 || k.fract() != 0.0 || n < 0.0 {
        return f64::NAN;
    }
    if k < 0.0 || k > n {
        return 0.0;
    }
    (0..k as u64).fold(1.0, |acc, i| acc * (n - i as f64))
}

fn median(a: &[f64]) -> f64 {
    let mut v = a.to_vec();
    v.sort_by(f64::total_cmp);
    let mid = v.len() / 2;
    if v.len() % 2 == 1 {
        v[mid]
    } else {
        (v[mid - 1] + v[mid]) / 2.0
    }
}

/// x! for any real x, through the gamma function, as Desmos allows.
pub fn factorial(x: f64) -> f64 {
    if x.fract() == 0.0 {
        if x < 0.0 {
            return f64::NAN;
        }
        if x <= 170.0 {
            return (1..=x as u64).fold(1.0, |acc, i| acc * i as f64);
        }
        return f64::INFINITY;
    }
    gamma(x + 1.0)
}

/// Lanczos approximation, accurate to about 15 digits.
fn gamma(x: f64) -> f64 {
    const G: f64 = 7.0;
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        return PI / ((PI * x).sin() * gamma(1.0 - x));
    }
    let x = x - 1.0;
    let mut a = C[0];
    let t = x + G + 0.5;
    for (i, c) in C.iter().enumerate().skip(1) {
        a += c / (x + i as f64);
    }
    (2.0 * PI).sqrt() * t.powf(x + 0.5) * (-t).exp() * a
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-10 * (1.0 + b.abs())
    }

    #[test]
    fn negative_bases_with_odd_roots_are_real() {
        assert!(close(pow(-8.0, 1.0 / 3.0), -2.0));
        assert!(close(pow(-8.0, 2.0 / 3.0), 4.0));
        assert!(pow(-8.0, 0.5).is_nan());
        assert!(close(pow(-2.0, 3.0), -8.0));
    }

    #[test]
    fn factorial_and_gamma() {
        assert_eq!(factorial(5.0), 120.0);
        assert!(close(factorial(0.5), 0.886_226_925_452_758));
        assert!(factorial(-1.0).is_nan());
    }

    #[test]
    fn desmos_mod_takes_sign_of_divisor() {
        assert_eq!(Mod.eval(&[-1.0, 3.0]), 2.0);
        assert_eq!(Mod.eval(&[7.0, 3.0]), 1.0);
    }

    #[test]
    fn integer_functions() {
        assert_eq!(Gcd.eval(&[12.0, 18.0]), 6.0);
        assert_eq!(Lcm.eval(&[4.0, 6.0]), 12.0);
        assert_eq!(NCr.eval(&[5.0, 2.0]), 10.0);
        assert_eq!(NPr.eval(&[5.0, 2.0]), 20.0);
        assert!(Gcd.eval(&[1.5, 3.0]).is_nan());
    }

    #[test]
    fn arccot_range_is_zero_to_pi() {
        assert!(close(Arccot.eval(&[-1.0]), 3.0 * PI / 4.0));
    }
}
